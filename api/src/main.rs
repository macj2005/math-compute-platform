use api::app_state::AppState;
use api::queue::PostgresJobQueue;
use api::router::build_router;
use sqlx::postgres::PgPoolOptions;
use tracing::info;

const LISTEN_ADDR: &str = "0.0.0.0:3000";

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

    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("failed to run database migrations");

    info!("database migrations completed");

    let app_state = AppState {
        job_queue: PostgresJobQueue::new(db_pool.clone()),
        db_pool,
    };

    let app = build_router(app_state);

    let listener = tokio::net::TcpListener::bind(LISTEN_ADDR)
        .await
        .expect("failed to bind API server to the given port.");

    info!("API server listening on http://{}", LISTEN_ADDR);

    axum::serve(listener, app).await.expect("API server failed");
}
