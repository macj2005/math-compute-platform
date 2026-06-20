mod api_error;
mod app_state;
mod health;
mod jobs;
mod metrics;
mod runner;
mod tasks;
mod worker;

use crate::app_state::AppState;
use crate::health::health_check;
use crate::jobs::{clear_jobs_endpoint, create_job, get_job, list_jobs, run_job_by_id};
use crate::metrics::get_metrics;
use crate::worker::start_worker_loop;
use axum::{
    Router,
    routing::{get, post},
};
use sqlx::postgres::PgPoolOptions;
use tracing::info;

const LISTEN_ADDR: &str = "127.0.0.1:3000";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter("api=debug,tower_http=debug")
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect to Postgres");

    info!("connected to Postgres");

    tokio::spawn(start_worker_loop(db_pool.clone()));
    let app_state = AppState { db_pool };

    // Add routes to API
    let app = Router::new()
        .route("/health", get(health_check))
        .route(
            "/jobs",
            post(create_job).get(list_jobs).delete(clear_jobs_endpoint),
        )
        .route("/jobs/:id", get(get_job))
        .route("/jobs/:id/run", post(run_job_by_id))
        .route("/metrics", get(get_metrics))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(LISTEN_ADDR)
        .await
        .expect("failed to bind API server to the given port.");

    info!("API server listening on http://{}", LISTEN_ADDR);

    axum::serve(listener, app).await.expect("API server failed");
}
