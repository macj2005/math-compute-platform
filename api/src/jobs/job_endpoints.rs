use axum::{
    Json,
    extract::{Path, State},
};
use chrono::Utc;
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::queue::JobQueue;
use crate::runner::{MONTE_CARLO_INTEGRATION_TASK, MONTE_CARLO_PI_TASK};
use crate::tasks::IntegrationInput;
use crate::worker::{ProcessJobError, process_job_by_id};

use super::{
    ClearJobsResponse, CreateJobRequest, CreateJobResponse, Job, JobDetails, JobStatus, clear_jobs,
    get_job_by_id, get_job_progress, insert_job, list_jobs_from_db,
};

// POST: create a new job
pub async fn create_job(
    State(state): State<AppState>,
    Json(request): Json<CreateJobRequest>,
) -> Result<Json<CreateJobResponse>, ApiError> {
    validate_job_input(&request)?;

    let job_id = Uuid::new_v4();

    info!(
        job_id = %job_id,
        task_type = request.task_type.as_str(),
        "received job creation request"
    );

    let job = Job {
        id: job_id,
        task_type: request.task_type,
        status: JobStatus::Pending,
        input: request.input,
        result: None,
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        retry_count: 0,
    };

    insert_job(&state.db_pool, &job).await.map_err(|error| {
        tracing::error!(%error, "failed to insert job into Postgres");
        ApiError::internal("failed to create job")
    })?;

    state.job_queue.enqueue(job_id).await.map_err(|error| {
        tracing::error!(%error, %job_id, "failed to enqueue job");
        ApiError::internal("failed to enqueue job")
    })?;

    Ok(Json(CreateJobResponse { job_id }))
}

fn validate_job_input(request: &CreateJobRequest) -> Result<(), ApiError> {
    match request.task_type.as_str() {
        MONTE_CARLO_PI_TASK => {
            let iterations = request
                .input
                .get("iterations")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| ApiError::bad_request("iterations must be a u64"))?;
            if iterations == 0 {
                return Err(ApiError::bad_request("iterations must be greater than 0"));
            }
            Ok(())
        }
        MONTE_CARLO_INTEGRATION_TASK => {
            let input = serde_json::from_value::<IntegrationInput>(request.input.clone()).map_err(
                |error| ApiError::bad_request(format!("invalid integration input: {error}")),
            )?;
            input.validate().map_err(ApiError::bad_request)
        }
        task_type => Err(ApiError::bad_request(format!(
            "unsupported task_type: {task_type}"
        ))),
    }
}

// POST: create a job that intentionally fails when a worker runs it.
// Manual testing helper for retry and DLQ behavior.
pub async fn create_failing_job(
    State(state): State<AppState>,
) -> Result<Json<CreateJobResponse>, ApiError> {
    let job_id = Uuid::new_v4();
    let task_type = "intentional_failure".to_string();
    let input = json!({
        "reason": "manual DLQ test",
    });

    info!(
        job_id = %job_id,
        task_type = task_type.as_str(),
        "received intentional failing job creation request"
    );

    let job = Job {
        id: job_id,
        task_type,
        status: JobStatus::Pending,
        input,
        result: None,
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        retry_count: 0,
    };

    insert_job(&state.db_pool, &job).await.map_err(|error| {
        tracing::error!(%error, "failed to insert failing job into Postgres");
        ApiError::internal("failed to create failing job")
    })?;

    state.job_queue.enqueue(job_id).await.map_err(|error| {
        tracing::error!(%error, %job_id, "failed to enqueue failing job");
        ApiError::internal("failed to enqueue failing job")
    })?;

    Ok(Json(CreateJobResponse { job_id }))
}

// GET: get a job by id
pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<JobDetails>, ApiError> {
    let job = get_job_by_id(&state.db_pool, job_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, %job_id, "failed to get job from Postgres");
            ApiError::internal("failed to get job")
        })?
        .ok_or_else(|| ApiError::not_found(format!("job not found: {job_id}")))?;
    let progress = get_job_progress(&state.db_pool, job_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, %job_id, "failed to get job progress from Postgres");
            ApiError::internal("failed to get job progress")
        })?;

    Ok(Json(JobDetails { job, progress }))
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
            ProcessJobError::WorkerTask => ApiError::internal("worker task failed"),
        })
}

#[cfg(test)]
#[path = "job_endpoints_tests.rs"]
mod tests;
