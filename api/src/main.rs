mod health;
mod jobs;

use std::{ collections::HashMap, sync::{Arc, Mutex} };
use crate::health::health_check;
use crate::jobs::{JobStore, create_job, get_job, list_jobs};
use axum::{ Router, routing::{get, post} };
use tracing::info;

const LISTEN_ADDR: &str = "127.0.0.1:3000";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("api=debug,tower_http=debug")
        .init();

    let job_store: JobStore = Arc::new(Mutex::new(HashMap::new()));

    // Add routes to API
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/jobs", post(create_job).get(list_jobs))
        .route("/jobs/:id", get(get_job))
        .with_state(job_store);

    let listener = tokio::net::TcpListener::bind(LISTEN_ADDR)
        .await
        .expect("failed to bind API server to the given port.");

    info!("API server listening on http://{}", LISTEN_ADDR);

    axum::serve(listener, app).await.expect("API server failed");
}
