use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct Job {
    pub(crate) id: String,
    pub(crate) task_type: String,
    pub(crate) status: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JobDetails {
    pub(crate) id: String,
    pub(crate) task_type: String,
    pub(crate) status: String,
    pub(crate) input: Value,
    pub(crate) result: Option<Value>,
    pub(crate) error: Option<String>,
    pub(crate) created_at: String,
    pub(crate) started_at: Option<String>,
    pub(crate) completed_at: Option<String>,
    pub(crate) retry_count: i32,
    pub(crate) progress: Option<JobProgress>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JobProgress {
    pub(crate) percent: f64,
    pub(crate) total_partitions: u64,
    pub(crate) pending_partitions: u64,
    pub(crate) running_partitions: u64,
    pub(crate) completed_partitions: u64,
    pub(crate) failed_partitions: u64,
    pub(crate) completed_samples: u64,
    pub(crate) total_samples: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Metrics {
    pub(crate) pending_jobs: usize,
    pub(crate) running_jobs: usize,
    pub(crate) completed_jobs: usize,
    pub(crate) failed_jobs: usize,
    pub(crate) total_jobs: usize,
}

#[derive(Serialize)]
struct CreateJobRequest<'a> {
    task_type: &'a str,
    input: Value,
}

#[derive(Deserialize)]
struct CreateJobResponse {
    job_id: String,
}

pub(crate) struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    pub(crate) fn new(base_url: String) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    pub(crate) async fn tasks_and_metrics(&self) -> Result<(Vec<Job>, Metrics), String> {
        let (jobs, metrics) = tokio::join!(
            self.client.get(format!("{}/jobs", self.base_url)).send(),
            self.client.get(format!("{}/metrics", self.base_url)).send()
        );

        let jobs = decode_response(jobs, "jobs").await?;
        let metrics = decode_response(metrics, "metrics").await?;
        Ok((jobs, metrics))
    }

    pub(crate) async fn create_job(&self, task_type: &str, input: Value) -> Result<String, String> {
        let response = self
            .client
            .post(format!("{}/jobs", self.base_url))
            .json(&CreateJobRequest { task_type, input })
            .send()
            .await;
        let created: CreateJobResponse = decode_response(response, "job creation").await?;
        Ok(created.job_id)
    }

    pub(crate) async fn job_details(&self, job_id: &str) -> Result<JobDetails, String> {
        let response = self
            .client
            .get(format!("{}/jobs/{job_id}", self.base_url))
            .send()
            .await;
        decode_response(response, "job details").await
    }
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: Result<reqwest::Response, reqwest::Error>,
    operation: &str,
) -> Result<T, String> {
    let response = response.map_err(|error| connection_error(error, operation))?;
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .map_err(|error| format!("The API returned an unexpected response: {error}"));
    }

    let message = response
        .text()
        .await
        .unwrap_or_else(|_| "no error details were returned".to_owned());
    Err(format_http_error(status, &message))
}

fn connection_error(error: reqwest::Error, operation: &str) -> String {
    if error.is_connect() {
        format!("Cannot connect to the API for {operation}; is it running?")
    } else {
        format!("{operation} request failed: {error}")
    }
}

pub(crate) fn format_http_error(status: StatusCode, body: &str) -> String {
    if let Ok(json) = serde_json::from_str::<Value>(body)
        && let Some(message) = json
            .get("error")
            .or_else(|| json.get("message"))
            .and_then(Value::as_str)
    {
        return format!("API returned {status}: {message}");
    }
    format!("API returned {status}: {}", body.trim())
}
