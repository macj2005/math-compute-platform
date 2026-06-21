use sqlx::PgPool;
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::sleep;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::jobs::{
    Job, JobResultUpdate, JobStatus, claim_job_by_id_from_db, get_job_by_id, update_job_result,
};
use crate::queue::{ActiveJobQueue, JobQueue, build_job_queue};
use crate::runner::run_job;

const DEFAULT_MAX_JOB_RETRIES: i32 = 3;
const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 1;
const DEFAULT_WORKER_CONCURRENCY: usize = 1;

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub max_retries: i32,
    pub poll_interval: Duration,
    pub concurrency: usize,
}

impl WorkerConfig {
    pub fn from_env() -> Self {
        Self {
            max_retries: read_i32_env("WORKER_MAX_RETRIES", DEFAULT_MAX_JOB_RETRIES),
            poll_interval: Duration::from_secs(read_u64_env(
                "WORKER_POLL_INTERVAL_SECONDS",
                DEFAULT_POLL_INTERVAL_SECONDS,
            )),
            concurrency: read_usize_env("WORKER_CONCURRENCY", DEFAULT_WORKER_CONCURRENCY),
        }
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_JOB_RETRIES,
            poll_interval: Duration::from_secs(DEFAULT_POLL_INTERVAL_SECONDS),
            concurrency: DEFAULT_WORKER_CONCURRENCY,
        }
    }
}

#[derive(Debug)]
pub enum ProcessJobError {
    NotFound,
    NotPending,
    Database,
}

pub async fn start_worker_loop(
    db_pool: PgPool,
    config: WorkerConfig,
    shutdown_signal: impl Future<Output = ()>,
) {
    let job_queue = build_job_queue(db_pool.clone())
        .await
        .expect("failed to configure job queue");

    info!(
        max_retries = config.max_retries,
        poll_interval_seconds = config.poll_interval.as_secs(),
        concurrency = config.concurrency,
        "background worker loop started"
    );

    let (shutdown_tx, _) = watch::channel(false);
    let mut worker_tasks = Vec::with_capacity(config.concurrency);

    for worker_task_id in 1..=config.concurrency {
        let task_db_pool = db_pool.clone();
        let task_job_queue = job_queue.clone();
        let task_config = config.clone();
        let task_shutdown_rx = shutdown_tx.subscribe();

        worker_tasks.push(tokio::spawn(async move {
            worker_task_loop(
                worker_task_id,
                task_db_pool,
                task_job_queue,
                task_config,
                task_shutdown_rx,
            )
            .await;
        }));
    }

    shutdown_signal.await;
    warn!("shutdown signal received - worker tasks stopped polling for new jobs");

    if shutdown_tx.send(true).is_err() {
        warn!("worker shutdown signal had no active receivers");
    }

    for worker_task in worker_tasks {
        if let Err(error) = worker_task.await {
            error!(?error, "worker task failed while shutting down");
        }
    }

    info!("worker loop shut down cleanly");
}

async fn worker_task_loop(
    worker_task_id: usize,
    db_pool: PgPool,
    job_queue: ActiveJobQueue,
    config: WorkerConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    info!(
        worker_task_id,
        poll_interval_seconds = config.poll_interval.as_secs(),
        "worker task started"
    );

    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        if let Some(job_id) =
            process_next_pending_job(db_pool.clone(), job_queue.clone(), &config, worker_task_id)
                .await
        {
            info!(
                %job_id,
                worker_task_id,
                poll_interval_seconds = config.poll_interval.as_secs(),
                "processed pending job - sleeping before next poll"
            );
        }

        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = sleep(config.poll_interval) => {}
        }
    }

    info!(worker_task_id, "worker task shut down cleanly");
}

pub async fn process_next_pending_job(
    db_pool: PgPool,
    job_queue: ActiveJobQueue,
    config: &WorkerConfig,
    worker_task_id: usize,
) -> Option<Uuid> {
    let queued_job = match job_queue.receive().await {
        Ok(Some(queued_job)) => queued_job,
        Ok(None) => return None,
        Err(error) => {
            error!(%error, worker_task_id, "failed to receive pending job from queue");
            return None;
        }
    };
    let job_to_run = queued_job.job.clone();

    let job_id = job_to_run.id;

    info!(%job_id, worker_task_id, "claimed pending job");

    match save_job_result(&db_pool, job_to_run, config, Some(worker_task_id)).await {
        Ok(JobStatus::Pending) => {
            if let Err(error) = job_queue.retry_later(&queued_job).await {
                error!(%job_id, %error, worker_task_id, "failed to leave job queued for retry");
            }
        }
        Ok(_) => {
            if let Err(error) = job_queue.complete(&queued_job).await {
                error!(%job_id, %error, worker_task_id, "failed to complete job in queue");
            }
        }
        Err(error) => {
            error!(%job_id, ?error, worker_task_id, "failed to finish pending job");
        }
    }

    Some(job_id)
}

pub async fn process_job_by_id(db_pool: PgPool, job_id: Uuid) -> Result<Job, ProcessJobError> {
    let job_to_run = claim_job_by_id_from_db(&db_pool, job_id)
        .await
        .map_err(|error| {
            error!(%job_id, %error, "failed to claim job from Postgres");
            ProcessJobError::Database
        })?;

    let Some(job_to_run) = job_to_run else {
        return match get_job_by_id(&db_pool, job_id).await {
            Ok(Some(_)) => Err(ProcessJobError::NotPending),
            Ok(None) => Err(ProcessJobError::NotFound),
            Err(error) => {
                error!(%job_id, %error, "failed to check job existence in Postgres");
                Err(ProcessJobError::Database)
            }
        };
    };

    save_job_result(&db_pool, job_to_run, &WorkerConfig::default(), None).await?;

    get_job_by_id(&db_pool, job_id)
        .await
        .map_err(|error| {
            error!(%job_id, %error, "failed to load completed job from Postgres");
            ProcessJobError::Database
        })?
        .ok_or(ProcessJobError::NotFound)
}

async fn save_job_result(
    db_pool: &PgPool,
    job_to_run: Job,
    config: &WorkerConfig,
    worker_task_id: Option<usize>,
) -> Result<JobStatus, ProcessJobError> {
    let job_id = job_to_run.id;
    let result = run_job(&job_to_run);

    let (status, output, error_message) = match result {
        Ok(output) => {
            info!(
                job_id = %job_to_run.id,
                task_type = job_to_run.task_type.as_str(),
                retry_count = job_to_run.retry_count,
                worker_task_id,
                result = %output,
                "job completed successfully"
            );

            (JobStatus::Completed, Some(output), None)
        }
        Err(error) => {
            let next_retry_count = job_to_run.retry_count + 1;
            let will_retry = next_retry_count <= config.max_retries;
            let next_status = if will_retry {
                JobStatus::Pending
            } else {
                JobStatus::Failed
            };

            error!(
                job_id = %job_to_run.id,
                task_type = job_to_run.task_type.as_str(),
                retry_count = next_retry_count,
                max_retries = config.max_retries,
                will_retry,
                worker_task_id,
                error = error.as_str(),
                "job failed"
            );

            (next_status, None, Some(error))
        }
    };

    let completed_at = if status == JobStatus::Pending {
        None
    } else {
        Some(chrono::Utc::now())
    };
    let retry_count = if status == JobStatus::Completed {
        job_to_run.retry_count
    } else {
        job_to_run.retry_count + 1
    };
    let update = JobResultUpdate {
        id: job_to_run.id,
        status: status.clone(),
        result: output,
        error: error_message,
        completed_at,
        retry_count,
    };

    update_job_result(db_pool, update).await.map_err(|error| {
        error!(%job_id, %error, "failed to save job result in Postgres");
        ProcessJobError::Database
    })?;

    info!(
        job_id = %job_to_run.id,
        retry_count,
        completed_at = ?completed_at,
        worker_task_id,
        "job processing finished"
    );

    Ok(status)
}

fn read_i32_env(key: &str, default_value: i32) -> i32 {
    let value = std::env::var(key).ok();
    parse_i32_env_value(value.as_deref(), default_value)
}

fn read_u64_env(key: &str, default_value: u64) -> u64 {
    let value = std::env::var(key).ok();
    parse_u64_env_value(value.as_deref(), default_value)
}

fn read_usize_env(key: &str, default_value: usize) -> usize {
    let value = std::env::var(key).ok();
    parse_usize_env_value(value.as_deref(), default_value)
}

fn parse_i32_env_value(value: Option<&str>, default_value: i32) -> i32 {
    value
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value >= 0)
        .unwrap_or(default_value)
}

fn parse_u64_env_value(value: Option<&str>, default_value: u64) -> u64 {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn parse_usize_env_value(value: Option<&str>, default_value: usize) -> usize {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

#[cfg(test)]
#[path = "worker_loop_tests.rs"]
mod tests;
