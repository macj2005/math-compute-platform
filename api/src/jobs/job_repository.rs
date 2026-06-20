use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{Job, JobStatus};

pub struct JobResultUpdate {
    pub id: Uuid,
    pub status: JobStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
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
            completed_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
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
    .execute(db_pool)
    .await?;

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
            completed_at
        FROM jobs
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .fetch_all(db_pool)
    .await?;

    rows.into_iter().map(job_from_row).collect()
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
            completed_at
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
            completed_at
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
            completed_at
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
            completed_at = $4
        WHERE id = $5
        "#,
    )
    .bind(update.status.as_str())
    .bind(update.result)
    .bind(update.error)
    .bind(update.completed_at)
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
    })
}
