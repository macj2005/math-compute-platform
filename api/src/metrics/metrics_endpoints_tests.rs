use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    jobs::{Job, JobStatus, insert_job},
    router::build_router,
};

#[tokio::test]
async fn get_metrics_returns_database_counts() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };

    let completed_job = test_job(Uuid::new_v4(), JobStatus::Completed);
    let failed_job = test_job(Uuid::new_v4(), JobStatus::Failed);
    let job_ids = [completed_job.id, failed_job.id];
    cleanup_jobs(&db_pool, &job_ids).await;

    let before = get_metrics_body(&db_pool).await;

    insert_job(&db_pool, &completed_job)
        .await
        .expect("completed insert should work");
    insert_job(&db_pool, &failed_job)
        .await
        .expect("failed insert should work");

    let after = get_metrics_body(&db_pool).await;

    assert_eq!(
        after["completed_jobs"].as_u64(),
        Some(before["completed_jobs"].as_u64().unwrap_or(0) + 1)
    );
    assert_eq!(
        after["failed_jobs"].as_u64(),
        Some(before["failed_jobs"].as_u64().unwrap_or(0) + 1)
    );
    assert_eq!(
        after["total_jobs"].as_u64(),
        Some(before["total_jobs"].as_u64().unwrap_or(0) + 2)
    );

    cleanup_jobs(&db_pool, &job_ids).await;
}

async fn get_metrics_body(db_pool: &PgPool) -> Value {
    let app = build_router(AppState {
        db_pool: db_pool.clone(),
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");

    serde_json::from_slice(&bytes).expect("body should be JSON")
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
        .expect("failed to run test database migrations");

    Some(db_pool)
}

fn test_job(id: Uuid, status: JobStatus) -> Job {
    Job {
        id,
        task_type: "monte_carlo_pi".to_string(),
        status,
        input: json!({ "iterations": 1000 }),
        result: Some(json!({ "pi_estimate": 3.14 })),
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: Some(Utc::now()),
        retry_count: 0,
    }
}

async fn cleanup_jobs(db_pool: &PgPool, job_ids: &[Uuid]) {
    for job_id in job_ids {
        sqlx::query("DELETE FROM jobs WHERE id = $1")
            .bind(job_id)
            .execute(db_pool)
            .await
            .expect("test cleanup should work");
    }
}
