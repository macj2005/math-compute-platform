use crate::queue::ActiveJobQueue;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub job_queue: ActiveJobQueue,
}
