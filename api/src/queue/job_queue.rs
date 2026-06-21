use async_trait::async_trait;
use std::fmt;
use uuid::Uuid;

use crate::jobs::Job;

#[derive(Clone, Debug)]
pub struct QueuedJob {
    pub job: Job,
    pub receipt_handle: Option<String>,
}

#[derive(Debug)]
pub enum JobQueueError {
    Database(sqlx::Error),
    AwsSdk(String),
    InvalidMessage(String),
    MissingReceiptHandle,
}

impl fmt::Display for JobQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobQueueError::Database(error) => write!(formatter, "database queue error: {error}"),
            JobQueueError::AwsSdk(error) => write!(formatter, "AWS SQS error: {error}"),
            JobQueueError::InvalidMessage(error) => {
                write!(formatter, "invalid queue message: {error}")
            }
            JobQueueError::MissingReceiptHandle => {
                write!(formatter, "queue message is missing receipt handle")
            }
        }
    }
}

impl std::error::Error for JobQueueError {}

impl From<sqlx::Error> for JobQueueError {
    fn from(error: sqlx::Error) -> Self {
        JobQueueError::Database(error)
    }
}

#[async_trait]
pub trait JobQueue: Clone + Send + Sync + 'static {
    async fn enqueue(&self, job_id: Uuid) -> Result<(), JobQueueError>;
    async fn receive(&self) -> Result<Option<QueuedJob>, JobQueueError>;
    async fn complete(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError>;
    async fn retry_later(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError>;
}
