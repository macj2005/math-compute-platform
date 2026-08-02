use super::{WorkerConfig, parse_i32_env_value, parse_u64_env_value, parse_usize_env_value};
use crate::jobs::{Job, JobStatus, get_job_by_id, get_job_progress, insert_job};
use crate::tasks::MONTE_CARLO_INTEGRATION_TASK;
use chrono::Utc;
use serde_json::json;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::time::Duration;
use uuid::Uuid;

#[test]
fn default_config_uses_expected_values() {
    let config = WorkerConfig::default();

    assert_eq!(config.max_retries, 3);
    assert_eq!(config.poll_interval, Duration::from_secs(1));
    assert_eq!(config.concurrency, 1);
}

#[test]
fn parses_valid_config_values() {
    assert_eq!(parse_i32_env_value(Some("5"), 3), 5);
    assert_eq!(parse_u64_env_value(Some("10"), 1), 10);
    assert_eq!(parse_usize_env_value(Some("4"), 1), 4);
}

#[test]
fn falls_back_to_defaults_for_invalid_config_values() {
    assert_eq!(parse_i32_env_value(Some("-1"), 3), 3);
    assert_eq!(parse_i32_env_value(Some("not-a-number"), 3), 3);
    assert_eq!(parse_u64_env_value(Some("0"), 1), 1);
    assert_eq!(parse_u64_env_value(None, 1), 1);
    assert_eq!(parse_usize_env_value(Some("0"), 1), 1);
    assert_eq!(parse_usize_env_value(None, 1), 1);
}

#[tokio::test]
async fn distributes_and_aggregates_integration_partitions() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let parent_id = Uuid::new_v4();
    cleanup_parent(&db_pool, parent_id).await;
    let parent = Job {
        id: parent_id,
        task_type: MONTE_CARLO_INTEGRATION_TASK.to_string(),
        status: JobStatus::Pending,
        input: json!({
            "expression": "x^2",
            "variables": ["x"],
            "bounds": [{ "min": 0.0, "max": 1.0 }],
            "samples": 20_000,
            "seed": 42,
            "partitions": 4
        }),
        result: None,
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        retry_count: 0,
    };
    insert_job(&db_pool, &parent)
        .await
        .expect("parent insert should work");

    let started_parent = super::process_job_by_id(db_pool.clone(), parent_id)
        .await
        .expect("parent should fan out");
    assert_eq!(started_parent.status, JobStatus::Running);

    let child_rows = sqlx::query(
        "SELECT id, partition_index FROM jobs WHERE parent_job_id = $1 ORDER BY partition_index",
    )
    .bind(parent_id)
    .fetch_all(&db_pool)
    .await
    .expect("children should load");
    assert_eq!(child_rows.len(), 4);

    let mut child_tasks = Vec::with_capacity(child_rows.len());
    for (expected_index, row) in child_rows.into_iter().enumerate() {
        let partition_index: i32 = row
            .try_get("partition_index")
            .expect("partition index should decode");
        let child_id: Uuid = row.try_get("id").expect("child id should decode");
        assert_eq!(partition_index, expected_index as i32);
        let child_db_pool = db_pool.clone();
        child_tasks.push(tokio::spawn(async move {
            super::process_job_by_id(child_db_pool, child_id).await
        }));
    }
    for child_task in child_tasks {
        child_task
            .await
            .expect("partition task should not panic")
            .expect("partition should complete");
    }

    let completed_parent = get_job_by_id(&db_pool, parent_id)
        .await
        .expect("parent should load")
        .expect("parent should exist");
    let estimate = completed_parent
        .result
        .as_ref()
        .and_then(|result| result["estimate"].as_f64())
        .expect("parent should contain an estimate");
    assert_eq!(completed_parent.status, JobStatus::Completed);
    assert!((estimate - 1.0 / 3.0).abs() < 0.01);

    let progress = get_job_progress(&db_pool, parent_id)
        .await
        .expect("progress should load")
        .expect("progress should exist");
    assert_eq!(progress.completed_partitions, 4);
    assert_eq!(progress.completed_samples, 20_000);
    assert_eq!(progress.percent, 100.0);

    cleanup_parent(&db_pool, parent_id).await;
}

#[tokio::test]
async fn failed_partition_fails_parent_and_cancels_pending_siblings() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let parent_id = Uuid::new_v4();
    cleanup_parent(&db_pool, parent_id).await;
    let parent = Job {
        id: parent_id,
        task_type: MONTE_CARLO_INTEGRATION_TASK.to_string(),
        status: JobStatus::Pending,
        input: json!({
            "expression": "1 / (x - x)",
            "variables": ["x"],
            "bounds": [{ "min": 0.0, "max": 1.0 }],
            "samples": 1_000,
            "seed": 42,
            "partitions": 4
        }),
        result: None,
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        retry_count: 0,
    };
    insert_job(&db_pool, &parent)
        .await
        .expect("parent insert should work");
    super::process_job_by_id(db_pool.clone(), parent_id)
        .await
        .expect("parent should fan out");

    let child_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE parent_job_id = $1 ORDER BY partition_index LIMIT 1",
    )
    .bind(parent_id)
    .fetch_one(&db_pool)
    .await
    .expect("child should load");

    for _ in 0..3 {
        super::process_job_by_id(db_pool.clone(), child_id)
            .await
            .expect("partition failure should be recorded");
    }

    let failed_parent = get_job_by_id(&db_pool, parent_id)
        .await
        .expect("parent should load")
        .expect("parent should exist");
    assert_eq!(failed_parent.status, JobStatus::Failed);
    assert!(
        failed_parent
            .error
            .as_deref()
            .is_some_and(|error| error.contains("non-finite"))
    );
    let pending_children: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE parent_job_id = $1 AND status = 'PENDING'",
    )
    .bind(parent_id)
    .fetch_one(&db_pool)
    .await
    .expect("pending child count should load");
    assert_eq!(pending_children, 0);

    cleanup_parent(&db_pool, parent_id).await;
}

async fn test_db_pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect to test Postgres");
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("failed to run test migrations");
    Some(db_pool)
}

async fn cleanup_parent(db_pool: &PgPool, parent_id: Uuid) {
    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(parent_id)
        .execute(db_pool)
        .await
        .expect("test cleanup should work");
}
