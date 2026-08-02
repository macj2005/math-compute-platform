use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "PENDING",
            JobStatus::Running => "RUNNING",
            JobStatus::Completed => "COMPLETED",
            JobStatus::Failed => "FAILED",
        }
    }

    pub fn from_str(status: &str) -> Option<Self> {
        match status {
            "PENDING" => Some(JobStatus::Pending),
            "RUNNING" => Some(JobStatus::Running),
            "COMPLETED" => Some(JobStatus::Completed),
            "FAILED" => Some(JobStatus::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub id: Uuid,
    pub task_type: String,
    pub status: JobStatus,
    pub input: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
}

#[derive(Debug, Serialize)]
pub struct JobProgress {
    pub percent: f64,
    pub total_partitions: u64,
    pub pending_partitions: u64,
    pub running_partitions: u64,
    pub completed_partitions: u64,
    pub failed_partitions: u64,
    pub completed_samples: u64,
    pub total_samples: u64,
}

#[derive(Debug, Serialize)]
pub struct JobDetails {
    #[serde(flatten)]
    pub job: Job,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<JobProgress>,
}

#[derive(Deserialize)]
pub struct CreateJobRequest {
    pub task_type: String,
    pub input: Value,
}

#[derive(Serialize)]
pub struct CreateJobResponse {
    pub job_id: Uuid,
}

#[derive(Serialize)]
pub struct ClearJobsResponse {
    pub deleted_jobs: u64,
}
