use sqlx::{PgPool, Row};

use super::metrics_types::MetricsResponse;

pub async fn get_metrics_from_db(db_pool: &PgPool) -> Result<MetricsResponse, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT status, COUNT(*) AS count
        FROM jobs
        GROUP BY status
        "#,
    )
    .fetch_all(db_pool)
    .await?;

    let mut metrics = MetricsResponse {
        pending_jobs: 0,
        running_jobs: 0,
        completed_jobs: 0,
        failed_jobs: 0,
        total_jobs: 0,
    };

    for row in rows {
        let status: String = row.try_get("status")?;
        let count: i64 = row.try_get("count")?;
        let count = count as usize;

        metrics.total_jobs += count;

        match status.as_str() {
            "PENDING" => metrics.pending_jobs = count,
            "RUNNING" => metrics.running_jobs = count,
            "COMPLETED" => metrics.completed_jobs = count,
            "FAILED" => metrics.failed_jobs = count,
            _ => {}
        }
    }

    Ok(metrics)
}
