use serde::Serialize;
#[derive(Serialize)]
pub struct MetricsResponse {
    pub pending_jobs: usize,
    pub running_jobs: usize,
    pub completed_jobs: usize,
    pub failed_jobs: usize,
    pub total_jobs: usize,
}
