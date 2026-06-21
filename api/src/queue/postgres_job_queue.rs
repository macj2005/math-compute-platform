use async_trait::async_trait;
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use crate::jobs::{Job, claim_next_pending_job_from_db};

use super::{JobQueue, JobQueueError};

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

    async fn receive(&self) -> Result<Option<Job>, JobQueueError> {
        claim_next_pending_job_from_db(&self.db_pool)
            .await
            .map_err(JobQueueError::from)
    }

    async fn complete(&self, job_id: Uuid) -> Result<(), JobQueueError> {
        debug!(%job_id, "job completed in Postgres-backed queue");
        Ok(())
    }
}
