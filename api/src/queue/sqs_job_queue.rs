use async_trait::async_trait;
use aws_sdk_sqs::Client;
use sqlx::PgPool;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::jobs::{JobStatus, claim_job_by_id_from_db, get_job_by_id};

use super::{JobQueue, JobQueueError, QueuedJob};

#[derive(Clone)]
pub struct SqsJobQueue {
    client: Client,
    db_pool: PgPool,
    queue_url: String,
}

impl SqsJobQueue {
    pub async fn from_env(db_pool: PgPool, queue_url: String) -> Self {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        let client = Client::new(&config);

        Self {
            client,
            db_pool,
            queue_url,
        }
    }

    async fn delete_message(&self, receipt_handle: &str) -> Result<(), JobQueueError> {
        self.client
            .delete_message()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
            .map_err(|error| JobQueueError::AwsSdk(error.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl JobQueue for SqsJobQueue {
    async fn enqueue(&self, job_id: Uuid) -> Result<(), JobQueueError> {
        self.client
            .send_message()
            .queue_url(&self.queue_url)
            .message_body(job_id.to_string())
            .send()
            .await
            .map_err(|error| JobQueueError::AwsSdk(error.to_string()))?;

        debug!(%job_id, "sent job to SQS queue");
        Ok(())
    }

    async fn receive(&self) -> Result<Option<QueuedJob>, JobQueueError> {
        let output = self
            .client
            .receive_message()
            .queue_url(&self.queue_url)
            .max_number_of_messages(1)
            .wait_time_seconds(1)
            .send()
            .await
            .map_err(|error| JobQueueError::AwsSdk(error.to_string()))?;

        let Some(message) = output.messages().first() else {
            return Ok(None);
        };

        let receipt_handle = message
            .receipt_handle()
            .ok_or(JobQueueError::MissingReceiptHandle)?
            .to_string();
        let body = message
            .body()
            .ok_or_else(|| JobQueueError::InvalidMessage("missing message body".to_string()))?;
        let job_id = Uuid::parse_str(body).map_err(|error| {
            JobQueueError::InvalidMessage(format!("message body is not a UUID: {error}"))
        })?;

        let Some(job) = claim_job_by_id_from_db(&self.db_pool, job_id)
            .await
            .map_err(JobQueueError::from)?
        else {
            match get_job_by_id(&self.db_pool, job_id)
                .await
                .map_err(JobQueueError::from)?
            {
                Some(job) if matches!(job.status, JobStatus::Completed | JobStatus::Failed) => {
                    self.delete_message(&receipt_handle).await?;
                    warn!(%job_id, "deleted stale SQS message for terminal job");
                }
                Some(job) => {
                    debug!(%job_id, status = job.status.as_str(), "SQS message did not claim a pending job");
                }
                None => {
                    self.delete_message(&receipt_handle).await?;
                    warn!(%job_id, "deleted SQS message for missing job");
                }
            }

            return Ok(None);
        };

        Ok(Some(QueuedJob {
            job,
            receipt_handle: Some(receipt_handle),
        }))
    }

    async fn complete(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError> {
        let receipt_handle = queued_job
            .receipt_handle
            .as_deref()
            .ok_or(JobQueueError::MissingReceiptHandle)?;

        self.delete_message(receipt_handle).await?;
        debug!(job_id = %queued_job.job.id, "deleted completed SQS message");

        Ok(())
    }

    async fn retry_later(&self, queued_job: &QueuedJob) -> Result<(), JobQueueError> {
        debug!(
            job_id = %queued_job.job.id,
            "leaving SQS message for retry after visibility timeout"
        );
        Ok(())
    }
}
