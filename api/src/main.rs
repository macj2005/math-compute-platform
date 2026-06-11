use axum::{
    Json, Router,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("api=debug,tower_http=debug")
        .init();

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/jobs", post(create_job));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind API server to 127.0.0.1:3000");

    info!("API server listening on http://127.0.0.1:3000");

    axum::serve(listener, app)
        .await
        .expect("API server failed");
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn create_job(Json(request): Json<CreateJobRequest>) -> Json<CreateJobResponse> {
    info!(
        task_type = request.task_type,
        iterations = request.input.iterations,
        "received job creation request"
    );

    Json(CreateJobResponse {
        job_id: Uuid::new_v4(),
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

#[derive(Deserialize)]
struct CreateJobRequest {
    task_type: String,
    input: MonteCarloPiInput,
}

#[derive(Deserialize)]
struct MonteCarloPiInput {
    iterations: u64,
}

#[derive(Serialize)]
struct CreateJobResponse {
    job_id: Uuid,
}
