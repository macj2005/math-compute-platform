use axum::{Json, extract::State};

use crate::jobs::{JobStatus, JobStore};
use crate::metrics::metrics_types::MetricsResponse;

pub async fn get_metrics(State(job_store): State<JobStore>) -> Json<MetricsResponse> {
    let jobs = job_store.lock().expect("job store lock poisoned");

    let mut metrics = MetricsResponse {
        pending_jobs: 0,
        running_jobs: 0,
        completed_jobs: 0,
        failed_jobs: 0,
        total_jobs: jobs.len(),
    };

    for job in jobs.values() {
        match job.status {
            JobStatus::Pending => metrics.pending_jobs += 1,
            JobStatus::Running => metrics.running_jobs += 1,
            JobStatus::Completed => metrics.completed_jobs += 1,
            JobStatus::Failed => metrics.failed_jobs += 1,
        }
    }

    Json(metrics)
}
