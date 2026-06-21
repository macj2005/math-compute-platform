pub mod api_error;
pub mod app_state;
pub mod health;
pub mod jobs;
pub mod metrics;
pub mod queue;
pub mod router;
pub mod runner;
pub mod tasks;
pub mod worker;

#[cfg(test)]
mod test_support {
    use std::sync::OnceLock;
    use tokio::sync::{Mutex, MutexGuard};

    static DB_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub async fn db_test_guard() -> MutexGuard<'static, ()> {
        DB_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await
    }
}
