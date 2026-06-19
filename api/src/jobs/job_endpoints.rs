use axum::{
    Form, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use crate::runner::run_job;

use super::{CreateJobForm, CreateJobResponse, Job, JobStatus, JobStore};

// POST: create a new job
pub async fn create_job(
    State(job_store): State<JobStore>,
    Form(form): Form<CreateJobForm>,
) -> Json<CreateJobResponse> {
    let job_id = Uuid::new_v4();
    let input = json!({
        "iterations": form.iterations,
    });

    info!(
        job_id = %job_id,
        task_type = form.task_type.as_str(),
        iterations = form.iterations,
        "received job creation request"
    );

    let job = Job {
        id: job_id,
        task_type: form.task_type,
        status: JobStatus::Pending,
        input,
        result: None,
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
    };

    job_store
        .lock()
        .expect("job store lock poisoned")
        .insert(job_id, job);

    Json(CreateJobResponse { job_id })
}

// GET: get a job by id
pub async fn get_job(
    State(job_store): State<JobStore>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<Job>, StatusCode> {
    let jobs = job_store.lock().expect("job store lock poisoned");

    jobs.get(&job_id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

// GET: list all jobs
pub async fn list_jobs(State(job_store): State<JobStore>) -> Json<Vec<Job>> {
    let jobs = job_store.lock().expect("job store lock poisoned");
    let mut jobs: Vec<Job> = jobs.values().cloned().collect();

    jobs.sort_by_key(|job| job.id);

    Json(jobs)
}

// POST: run a job by id
// Used for test purposes
pub async fn run_job_by_id(
    State(job_store): State<JobStore>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<Job>, StatusCode> {
    let job_to_run = {
        let mut jobs = job_store.lock().expect("job store lock poisoned");
        let job = jobs.get_mut(&job_id).ok_or(StatusCode::NOT_FOUND)?;

        if job.status != JobStatus::Pending {
            return Err(StatusCode::CONFLICT);
        }

        job.status = JobStatus::Running;
        job.started_at = Some(Utc::now());
        job.error = None;

        job.clone()
    };

    let result = run_job(&job_to_run);

    let mut jobs = job_store.lock().expect("job store lock poisoned");
    let job = jobs.get_mut(&job_id).ok_or(StatusCode::NOT_FOUND)?;

    match result {
        Ok(output) => {
            job.status = JobStatus::Completed;
            job.result = Some(output);
            job.error = None;
        }
        Err(error) => {
            job.status = JobStatus::Failed;
            job.result = None;
            job.error = Some(error);
        }
    }

    job.completed_at = Some(Utc::now());

    Ok(Json(job.clone()))
}
