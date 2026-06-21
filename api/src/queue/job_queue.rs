use async_trait::async_trait;
use std::fmt;
use uuid::Uuid;

use crate::jobs::Job;

#[derive(Debug)]
pub enum JobQueueError {
    Database(sqlx::Error),
}

impl fmt::Display for JobQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobQueueError::Database(error) => write!(formatter, "database queue error: {error}"),
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
    async fn receive(&self) -> Result<Option<Job>, JobQueueError>;
    async fn complete(&self, job_id: Uuid) -> Result<(), JobQueueError>;
}
