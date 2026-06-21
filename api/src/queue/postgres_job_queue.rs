use async_trait::async_trait;
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use crate::jobs::claim_next_pending_job_from_db;

use super::{JobQueue, JobQueueError, QueuedJob};

#[derive(Clone)]
pub struct PostgresJobQueue {
    db_pool: PgPool,
}

impl PostgresJobQueue {
    pub fn new(db_pool: PgPool) -> Self {
        Self { db_pool }
    }
}

#[async_trait]
impl JobQueue for PostgresJobQueue {
    async fn enqueue(&self, job_id: Uuid) -> Result<(), JobQueueError> {
        debug!(%job_id, "job is available in Postgres-backed queue");
        Ok(())
    }

    async fn receive(&self) -> Result<Option<QueuedJob>, JobQueueError> {
        let job = claim_next_pending_job_from_db(&self.db_pool)
            .await
            .map_err(JobQueueError::from)?;

        Ok(job.map(|job| QueuedJob {
            job,
            receipt_handle: None,
        }))
    }

    async fn complete(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError> {
        debug!(job_id = %queued_job.job.id, "job completed in Postgres-backed queue");
        Ok(())
    }

    async fn retry_later(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError> {
        debug!(job_id = %queued_job.job.id, "job returned to Postgres-backed queue");
        Ok(())
    }
}
