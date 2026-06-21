use axum::{
    Router,
    routing::{get, post},
};

use crate::app_state::AppState;
use crate::health::{health_check, ready_check};
use crate::jobs::{clear_jobs_endpoint, create_job, get_job, list_jobs, run_job_by_id};
use crate::metrics::get_metrics;

pub fn build_router(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(ready_check))
        .route(
            "/jobs",
            post(create_job).get(list_jobs).delete(clear_jobs_endpoint),
        )
        .route("/jobs/:id", get(get_job))
        .route("/jobs/:id/run", post(run_job_by_id))
        .route("/metrics", get(get_metrics))
        .with_state(app_state)
}
