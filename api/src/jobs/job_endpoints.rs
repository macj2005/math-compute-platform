use axum::{
    Form, Json,
    extract::{Path, State},
};
use chrono::Utc;
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::runner::MONTE_CARLO_PI_TASK;
use crate::worker::{ProcessJobError, process_job_by_id};

use super::{
    ClearJobsResponse, CreateJobForm, CreateJobResponse, Job, JobStatus, clear_jobs, get_job_by_id,
    insert_job, list_jobs_from_db,
};

// POST: create a new job
pub async fn create_job(
    State(state): State<AppState>,
    Form(form): Form<CreateJobForm>,
) -> Result<Json<CreateJobResponse>, ApiError> {
    if form.task_type != MONTE_CARLO_PI_TASK {
        return Err(ApiError::bad_request(format!(
            "unsupported task_type: {}",
            form.task_type
        )));
    }

    if form.iterations == 0 {
        return Err(ApiError::bad_request("iterations must be greater than 0"));
    }

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

    insert_job(&state.db_pool, &job).await.map_err(|error| {
        tracing::error!(%error, "failed to insert job into Postgres");
        ApiError::internal("failed to create job")
    })?;

    Ok(Json(CreateJobResponse { job_id }))
}

// GET: get a job by id
pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<Job>, ApiError> {
    get_job_by_id(&state.db_pool, job_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, %job_id, "failed to get job from Postgres");
            ApiError::internal("failed to get job")
        })?
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("job not found: {job_id}")))
}

// GET: list all jobs
pub async fn list_jobs(State(state): State<AppState>) -> Result<Json<Vec<Job>>, ApiError> {
    let jobs = list_jobs_from_db(&state.db_pool).await.map_err(|error| {
        tracing::error!(%error, "failed to list jobs from Postgres");
        ApiError::internal("failed to list jobs")
    })?;

    Ok(Json(jobs))
}

// DELETE: clear all jobs
pub async fn clear_jobs_endpoint(
    State(state): State<AppState>,
) -> Result<Json<ClearJobsResponse>, ApiError> {
    let deleted_jobs = clear_jobs(&state.db_pool).await.map_err(|error| {
        tracing::error!(%error, "failed to clear jobs from Postgres");
        ApiError::internal("failed to clear jobs")
    })?;

    Ok(Json(ClearJobsResponse { deleted_jobs }))
}

// POST: run a job by id
// Used for test purposes
pub async fn run_job_by_id(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<Job>, ApiError> {
    process_job_by_id(state.db_pool, job_id)
        .await
        .map(Json)
        .map_err(|error| match error {
            ProcessJobError::NotFound => ApiError::not_found(format!("job not found: {job_id}")),
            ProcessJobError::NotPending => {
                ApiError::conflict(format!("job is not pending and cannot be run: {job_id}"))
            }
            ProcessJobError::Database => ApiError::internal("failed to run job"),
        })
}
