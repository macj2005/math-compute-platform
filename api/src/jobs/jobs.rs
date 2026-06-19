use axum::{
    Form, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::json;
use tracing::info;
use uuid::Uuid;

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
