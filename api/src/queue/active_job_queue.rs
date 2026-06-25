use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use super::{JobQueue, JobQueueError, PostgresJobQueue, QueuedJob, SqsJobQueue};

#[derive(Clone)]
pub enum ActiveJobQueue {
    Postgres(PostgresJobQueue),
    Sqs(SqsJobQueue),
}

impl ActiveJobQueue {
    pub fn postgres(db_pool: PgPool) -> Self {
        Self::Postgres(PostgresJobQueue::new(db_pool))
    }

    pub fn sqs(queue: SqsJobQueue) -> Self {
        Self::Sqs(queue)
    }
}

#[async_trait]
impl JobQueue for ActiveJobQueue {
    async fn enqueue(&self, job_id: Uuid) -> Result<(), JobQueueError> {
        match self {
            ActiveJobQueue::Postgres(queue) => queue.enqueue(job_id).await,
            ActiveJobQueue::Sqs(queue) => queue.enqueue(job_id).await,
        }
    }

    async fn receive(&self) -> Result<Option<QueuedJob>, JobQueueError> {
        match self {
            ActiveJobQueue::Postgres(queue) => queue.receive().await,
            ActiveJobQueue::Sqs(queue) => queue.receive().await,
        }
    }

    async fn complete(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError> {
        match self {
            ActiveJobQueue::Postgres(queue) => queue.complete(queued_job).await,
            ActiveJobQueue::Sqs(queue) => queue.complete(queued_job).await,
        }
    }

    async fn dead_letter(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError> {
        match self {
            ActiveJobQueue::Postgres(queue) => queue.dead_letter(queued_job).await,
            ActiveJobQueue::Sqs(queue) => queue.dead_letter(queued_job).await,
        }
    }

    async fn retry_later(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError> {
        match self {
            ActiveJobQueue::Postgres(queue) => queue.retry_later(queued_job).await,
            ActiveJobQueue::Sqs(queue) => queue.retry_later(queued_job).await,
        }
    }
}
