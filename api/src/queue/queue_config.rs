use sqlx::PgPool;
use std::fmt;

use super::{ActiveJobQueue, SqsJobQueue};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobQueueBackend {
    Postgres,
    Sqs,
}

impl JobQueueBackend {
    pub fn from_env() -> Result<Self, JobQueueConfigError> {
        let value = std::env::var("JOB_QUEUE_BACKEND").unwrap_or_else(|_| "postgres".to_string());
        Self::parse(&value)
    }

    fn parse(value: &str) -> Result<Self, JobQueueConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "postgres" => Ok(Self::Postgres),
            "sqs" => Ok(Self::Sqs),
            backend => Err(JobQueueConfigError::UnknownBackend(backend.to_string())),
        }
    }
}

#[derive(Debug)]
pub enum JobQueueConfigError {
    UnknownBackend(String),
    MissingEnv(String),
}

impl fmt::Display for JobQueueConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobQueueConfigError::UnknownBackend(backend) => {
                write!(formatter, "unknown job queue backend: {backend}")
            }
            JobQueueConfigError::MissingEnv(key) => {
                write!(formatter, "missing required environment variable: {key}")
            }
        }
    }
}

impl std::error::Error for JobQueueConfigError {}

pub async fn build_job_queue(db_pool: PgPool) -> Result<ActiveJobQueue, JobQueueConfigError> {
    match JobQueueBackend::from_env()? {
        JobQueueBackend::Postgres => Ok(ActiveJobQueue::postgres(db_pool)),
        JobQueueBackend::Sqs => {
            let queue_url = std::env::var("SQS_QUEUE_URL")
                .map_err(|_| JobQueueConfigError::MissingEnv("SQS_QUEUE_URL".to_string()))?;
            let dead_letter_queue_url = std::env::var("SQS_DLQ_URL").ok();
            Ok(ActiveJobQueue::sqs(
                SqsJobQueue::from_env(db_pool, queue_url, dead_letter_queue_url).await,
            ))
        }
    }
}

#[cfg(test)]
#[path = "queue_config_tests.rs"]
mod tests;
