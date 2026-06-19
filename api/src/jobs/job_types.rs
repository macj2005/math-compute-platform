use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub type JobStore = Arc<Mutex<HashMap<Uuid, Job>>>;

#[derive(Clone, Debug, Serialize)]
#[allow(dead_code)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub id: Uuid,
    pub task_type: String,
    pub status: JobStatus,
    pub input: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateJobForm { // to use URL encoding for API requests
    pub task_type: String,
    pub iterations: u64,
}

#[derive(Serialize)]
pub struct CreateJobResponse {
    pub job_id: Uuid,
}
