use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;

use crate::{app_state::AppState, queue::PostgresJobQueue, router::build_router};

#[tokio::test]
async fn health_returns_ok() {
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let app = build_router(test_app_state(db_pool));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["status"], "ok");
}

#[tokio::test]
async fn ready_returns_ready_when_database_is_reachable() {
    let Some(db_pool) = test_db_pool().await else {
        return;
    };
    let app = build_router(test_app_state(db_pool));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ready")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should work");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["status"], "ready");
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

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");

    serde_json::from_slice(&bytes).expect("body should be JSON")
}

fn test_app_state(db_pool: PgPool) -> AppState {
    AppState {
        job_queue: PostgresJobQueue::new(db_pool.clone()),
        db_pool,
    }
}
