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
    queue::PostgresJobQueue,
    router::build_router,
};

#[tokio::test]
async fn post_jobs_creates_job() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let app = build_router(test_app_state(db_pool.clone()));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jobs")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("task_type=monte_carlo_pi&iterations=1000"))
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::OK);

    let body = response_json(response).await;
    let job_id = body
        .get("job_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("response should include job_id");

    let saved_job = crate::jobs::get_job_by_id(&db_pool, job_id)
        .await
        .expect("get should work")
        .expect("job should exist");

    assert_eq!(saved_job.task_type, "monte_carlo_pi");
    assert_eq!(saved_job.input, json!({ "iterations": 1000 }));

    cleanup_jobs(&db_pool, &[job_id]).await;
}

#[tokio::test]
async fn post_jobs_rejects_invalid_task_type() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let app = build_router(test_app_state(db_pool));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jobs")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("task_type=bad_task&iterations=1000"))
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_jobs_rejects_zero_iterations() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let app = build_router(test_app_state(db_pool));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jobs")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("task_type=monte_carlo_pi&iterations=0"))
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_jobs_by_id_returns_job() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let job = test_job(Uuid::new_v4());
    cleanup_jobs(&db_pool, &[job.id]).await;
    insert_job(&db_pool, &job)
        .await
        .expect("insert should work");

    let app = build_router(test_app_state(db_pool.clone()));
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/jobs/{}", job.id))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::OK);

    let body = response_json(response).await;
    assert_eq!(body["id"], job.id.to_string());
    assert_eq!(body["task_type"], "monte_carlo_pi");

    cleanup_jobs(&db_pool, &[job.id]).await;
}

#[tokio::test]
async fn get_jobs_by_id_returns_not_found_for_missing_job() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let app = build_router(test_app_state(db_pool));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/jobs/{}", Uuid::new_v4()))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
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

fn test_job(id: Uuid) -> Job {
    Job {
        id,
        task_type: "monte_carlo_pi".to_string(),
        status: JobStatus::Completed,
        input: json!({ "iterations": 1000 }),
        result: Some(json!({ "pi_estimate": 3.14 })),
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: Some(Utc::now()),
        retry_count: 0,
    }
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");

    serde_json::from_slice(&bytes).expect("body should be JSON")
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

fn test_app_state(db_pool: PgPool) -> AppState {
    AppState {
        job_queue: PostgresJobQueue::new(db_pool.clone()),
        db_pool,
    }
}
