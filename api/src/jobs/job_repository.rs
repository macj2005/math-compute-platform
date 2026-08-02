use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{Job, JobProgress, JobStatus};
use crate::tasks::{IntegrationInput, PartialStatistics, finalize_result};

pub struct JobResultUpdate {
    pub id: Uuid,
    pub status: JobStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
}

pub struct NewJobPartition {
    pub job: Job,
    pub parent_job_id: Uuid,
    pub partition_index: i32,
}

#[derive(Debug)]
pub enum PartitionResultError {
    Database(sqlx::Error),
    InvalidData(String),
}

impl std::fmt::Display for PartitionResultError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "partition database error: {error}"),
            Self::InvalidData(error) => write!(formatter, "invalid partition data: {error}"),
        }
    }
}

impl std::error::Error for PartitionResultError {}

impl From<sqlx::Error> for PartitionResultError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

pub async fn insert_job(db_pool: &PgPool, job: &Job) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO jobs (
            id,
            task_type,
            status,
            input,
            result,
            error,
            created_at,
            started_at,
            completed_at,
            retry_count
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(job.id)
    .bind(job.task_type.as_str())
    .bind(job.status.as_str())
    .bind(&job.input)
    .bind(&job.result)
    .bind(&job.error)
    .bind(job.created_at)
    .bind(job.started_at)
    .bind(job.completed_at)
    .bind(job.retry_count)
    .execute(db_pool)
    .await?;

    Ok(())
}

pub async fn ensure_job_partitions(
    db_pool: &PgPool,
    partitions: &[NewJobPartition],
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut transaction = db_pool.begin().await?;

    for partition in partitions {
        let job = &partition.job;
        sqlx::query(
            r#"
            INSERT INTO jobs (
                id, task_type, status, input, result, error, created_at, started_at,
                completed_at, retry_count, parent_job_id, partition_index
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (parent_job_id, partition_index) DO NOTHING
            "#,
        )
        .bind(job.id)
        .bind(job.task_type.as_str())
        .bind(job.status.as_str())
        .bind(&job.input)
        .bind(&job.result)
        .bind(&job.error)
        .bind(job.created_at)
        .bind(job.started_at)
        .bind(job.completed_at)
        .bind(job.retry_count)
        .bind(partition.parent_job_id)
        .bind(partition.partition_index)
        .execute(&mut *transaction)
        .await?;
    }

    let parent_job_id = partitions
        .first()
        .map(|partition| partition.parent_job_id)
        .ok_or_else(|| sqlx::Error::Protocol("at least one partition is required".to_string()))?;
    let rows = sqlx::query(
        r#"
        SELECT id
        FROM jobs
        WHERE parent_job_id = $1
        ORDER BY partition_index ASC
        "#,
    )
    .bind(parent_job_id)
    .fetch_all(&mut *transaction)
    .await?;

    transaction.commit().await?;
    rows.into_iter().map(|row| row.try_get("id")).collect()
}

pub async fn reset_running_job_to_pending(
    db_pool: &PgPool,
    job_id: Uuid,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'PENDING', error = $1
        WHERE id = $2 AND status = 'RUNNING'
        "#,
    )
    .bind(error)
    .bind(job_id)
    .execute(db_pool)
    .await?;

    Ok(())
}

pub async fn update_integration_partition_result(
    db_pool: &PgPool,
    update: JobResultUpdate,
) -> Result<(), PartitionResultError> {
    let mut transaction = db_pool.begin().await?;
    let child_status = update.status.clone();
    let child_error = update.error.clone();

    // Every partition transaction locks its parent before locking any child row.
    // This consistent ordering prevents concurrent workers from deadlocking while
    // one holds child A and waits for child B while another does the reverse.
    let parent_job_id: Uuid = sqlx::query_scalar("SELECT parent_job_id FROM jobs WHERE id = $1")
        .bind(update.id)
        .fetch_one(&mut *transaction)
        .await?;
    let parent_row = sqlx::query(
        r#"
        SELECT status, input
        FROM jobs
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(parent_job_id)
    .fetch_one(&mut *transaction)
    .await?;

    sqlx::query(
        r#"
        UPDATE jobs
        SET status = $1,
            result = $2,
            error = $3,
            completed_at = $4,
            retry_count = $5
        WHERE id = $6
        "#,
    )
    .bind(update.status.as_str())
    .bind(update.result)
    .bind(update.error)
    .bind(update.completed_at)
    .bind(update.retry_count)
    .bind(update.id)
    .execute(&mut *transaction)
    .await?;

    if child_status == JobStatus::Pending {
        transaction.commit().await?;
        return Ok(());
    }

    let parent_status: String = parent_row.try_get("status")?;

    if parent_status != JobStatus::Running.as_str() {
        transaction.commit().await?;
        return Ok(());
    }

    if child_status == JobStatus::Failed {
        let error = format!(
            "integration partition failed: {}",
            child_error.unwrap_or_else(|| "unknown error".to_string())
        );
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'FAILED', error = $1, completed_at = $2
            WHERE id = $3 AND status = 'RUNNING'
            "#,
        )
        .bind(error)
        .bind(Utc::now())
        .bind(parent_job_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'FAILED',
                error = 'cancelled because another integration partition failed',
                completed_at = $1
            WHERE parent_job_id = $2 AND status = 'PENDING'
            "#,
        )
        .bind(Utc::now())
        .bind(parent_job_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(());
    }

    let child_rows = sqlx::query(
        r#"
        SELECT status, result
        FROM jobs
        WHERE parent_job_id = $1
        ORDER BY partition_index ASC
        FOR UPDATE
        "#,
    )
    .bind(parent_job_id)
    .fetch_all(&mut *transaction)
    .await?;

    if child_rows.iter().any(|row| {
        row.try_get::<String, _>("status")
            .map(|status| status != JobStatus::Completed.as_str())
            .unwrap_or(true)
    }) {
        transaction.commit().await?;
        return Ok(());
    }

    let input_value: Value = parent_row.try_get("input")?;
    let input = serde_json::from_value::<IntegrationInput>(input_value)
        .map_err(|error| PartitionResultError::InvalidData(error.to_string()))?;
    let mut combined = PartialStatistics::empty();
    for row in child_rows {
        let result: Value = row.try_get("result")?;
        let partial = serde_json::from_value::<PartialStatistics>(result)
            .map_err(|error| PartitionResultError::InvalidData(error.to_string()))?;
        combined = combined.merge(partial);
    }
    let result = finalize_result(&input, combined).map_err(PartitionResultError::InvalidData)?;
    let result = serde_json::to_value(result)
        .map_err(|error| PartitionResultError::InvalidData(error.to_string()))?;

    sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'COMPLETED', result = $1, error = NULL, completed_at = $2
        WHERE id = $3 AND status = 'RUNNING'
        "#,
    )
    .bind(result)
    .bind(Utc::now())
    .bind(parent_job_id)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(())
}

pub async fn list_jobs_from_db(db_pool: &PgPool) -> Result<Vec<Job>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            id,
            task_type,
            status,
            input,
            result,
            error,
            created_at,
            started_at,
            completed_at,
            retry_count
        FROM jobs
        WHERE parent_job_id IS NULL
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .fetch_all(db_pool)
    .await?;

    rows.into_iter().map(job_from_row).collect()
}

pub async fn get_job_progress(
    db_pool: &PgPool,
    parent_job_id: Uuid,
) -> Result<Option<JobProgress>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(*) AS total_partitions,
            COUNT(*) FILTER (WHERE status = 'PENDING') AS pending_partitions,
            COUNT(*) FILTER (WHERE status = 'RUNNING') AS running_partitions,
            COUNT(*) FILTER (WHERE status = 'COMPLETED') AS completed_partitions,
            COUNT(*) FILTER (WHERE status = 'FAILED') AS failed_partitions,
            COALESCE(SUM((input->>'sample_count')::BIGINT)
                FILTER (WHERE status = 'COMPLETED'), 0)::BIGINT AS completed_samples,
            COALESCE(SUM((input->>'sample_count')::BIGINT), 0)::BIGINT AS total_samples
        FROM jobs
        WHERE parent_job_id = $1
        "#,
    )
    .bind(parent_job_id)
    .fetch_one(db_pool)
    .await?;

    let total_partitions: i64 = row.try_get("total_partitions")?;
    if total_partitions == 0 {
        return Ok(None);
    }
    let completed_samples: i64 = row.try_get("completed_samples")?;
    let total_samples: i64 = row.try_get("total_samples")?;
    let percent = if total_samples == 0 {
        0.0
    } else {
        completed_samples as f64 * 100.0 / total_samples as f64
    };

    Ok(Some(JobProgress {
        percent,
        total_partitions: total_partitions as u64,
        pending_partitions: row.try_get::<i64, _>("pending_partitions")? as u64,
        running_partitions: row.try_get::<i64, _>("running_partitions")? as u64,
        completed_partitions: row.try_get::<i64, _>("completed_partitions")? as u64,
        failed_partitions: row.try_get::<i64, _>("failed_partitions")? as u64,
        completed_samples: completed_samples as u64,
        total_samples: total_samples as u64,
    }))
}

pub async fn get_job_by_id(db_pool: &PgPool, job_id: Uuid) -> Result<Option<Job>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            id,
            task_type,
            status,
            input,
            result,
            error,
            created_at,
            started_at,
            completed_at,
            retry_count
        FROM jobs
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .fetch_optional(db_pool)
    .await?;

    row.map(job_from_row).transpose()
}

pub async fn clear_jobs(db_pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM jobs").execute(db_pool).await?;

    Ok(result.rows_affected())
}

pub async fn claim_next_pending_job_from_db(db_pool: &PgPool) -> Result<Option<Job>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'RUNNING',
            started_at = $1,
            error = NULL
        WHERE id = (
            SELECT id
            FROM jobs
            WHERE status = 'PENDING'
            ORDER BY created_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING
            id,
            task_type,
            status,
            input,
            result,
            error,
            created_at,
            started_at,
            completed_at,
            retry_count
        "#,
    )
    .bind(Utc::now())
    .fetch_optional(db_pool)
    .await?;

    row.map(job_from_row).transpose()
}

pub async fn claim_job_by_id_from_db(
    db_pool: &PgPool,
    job_id: Uuid,
) -> Result<Option<Job>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'RUNNING',
            started_at = $1,
            error = NULL
        WHERE id = $2
          AND status = 'PENDING'
        RETURNING
            id,
            task_type,
            status,
            input,
            result,
            error,
            created_at,
            started_at,
            completed_at,
            retry_count
        "#,
    )
    .bind(Utc::now())
    .bind(job_id)
    .fetch_optional(db_pool)
    .await?;

    row.map(job_from_row).transpose()
}

pub async fn update_job_result(
    db_pool: &PgPool,
    update: JobResultUpdate,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE jobs
        SET status = $1,
            result = $2,
            error = $3,
            completed_at = $4,
            retry_count = $5
        WHERE id = $6
        "#,
    )
    .bind(update.status.as_str())
    .bind(update.result)
    .bind(update.error)
    .bind(update.completed_at)
    .bind(update.retry_count)
    .bind(update.id)
    .execute(db_pool)
    .await?;

    Ok(())
}

fn job_from_row(row: sqlx::postgres::PgRow) -> Result<Job, sqlx::Error> {
    let status: String = row.try_get("status")?;
    let status = JobStatus::from_str(&status).ok_or_else(|| sqlx::Error::ColumnDecode {
        index: "status".to_string(),
        source: format!("unknown job status: {status}").into(),
    })?;

    Ok(Job {
        id: row.try_get("id")?,
        task_type: row.try_get("task_type")?,
        status,
        input: row.try_get("input")?,
        result: row.try_get("result")?,
        error: row.try_get("error")?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        retry_count: row.try_get("retry_count")?,
    })
}

#[cfg(test)]
#[path = "job_repository_tests.rs"]
mod tests;
