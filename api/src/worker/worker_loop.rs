use sqlx::PgPool;
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinError;
use tokio::time::sleep;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::jobs::{
    Job, JobResultUpdate, JobStatus, NewJobPartition, claim_job_by_id_from_db,
    ensure_job_partitions, get_job_by_id, reset_running_job_to_pending,
    update_integration_partition_result, update_job_result,
};
use crate::queue::{ActiveJobQueue, JobQueue, build_job_queue};
use crate::runner::run_job;
use crate::tasks::{
    IntegrationInput, IntegrationPartitionInput, MONTE_CARLO_INTEGRATION_PARTITION_TASK,
    MONTE_CARLO_INTEGRATION_TASK, partition_samples,
};

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
    WorkerTask,
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

    match save_job_result(
        &db_pool,
        &job_queue,
        job_to_run,
        config,
        Some(worker_task_id),
    )
    .await
    {
        Ok(JobStatus::Pending) => {
            if let Err(error) = job_queue.retry_later(&queued_job).await {
                error!(%job_id, %error, worker_task_id, "failed to leave job queued for retry");
            }
        }
        Ok(JobStatus::Failed) => {
            if let Err(error) = job_queue.dead_letter(&queued_job).await {
                error!(%job_id, %error, worker_task_id, "failed to move job to dead-letter queue");
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

    let job_queue = ActiveJobQueue::postgres(db_pool.clone());
    save_job_result(
        &db_pool,
        &job_queue,
        job_to_run,
        &WorkerConfig::default(),
        None,
    )
    .await?;

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
    job_queue: &ActiveJobQueue,
    job_to_run: Job,
    config: &WorkerConfig,
    worker_task_id: Option<usize>,
) -> Result<JobStatus, ProcessJobError> {
    if job_to_run.task_type == MONTE_CARLO_INTEGRATION_TASK {
        return fan_out_integration_job(db_pool, job_queue, &job_to_run).await;
    }

    let job_id = job_to_run.id;
    let task_type = job_to_run.task_type.clone();
    let retry_count = job_to_run.retry_count;
    let result = run_job_blocking(job_to_run.clone())
        .await
        .map_err(|error| {
            error!(%job_id, %error, "blocking job task failed");
            ProcessJobError::WorkerTask
        })?;

    let (status, output, error_message) = match result {
        Ok(output) => {
            info!(
                %job_id,
                task_type = task_type.as_str(),
                retry_count,
                worker_task_id,
                result = %output,
                "job completed successfully"
            );

            (JobStatus::Completed, Some(output), None)
        }
        Err(error) => {
            let next_retry_count = retry_count + 1;
            let will_retry = next_retry_count < config.max_retries;
            let next_status = if will_retry {
                JobStatus::Pending
            } else {
                JobStatus::Failed
            };

            error!(
                %job_id,
                task_type = task_type.as_str(),
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
        retry_count
    } else {
        retry_count + 1
    };
    let update = JobResultUpdate {
        id: job_id,
        status: status.clone(),
        result: output,
        error: error_message,
        completed_at,
        retry_count,
    };

    if task_type == MONTE_CARLO_INTEGRATION_PARTITION_TASK {
        if let Err(error) = update_integration_partition_result(db_pool, update).await {
            let retry_message = format!("failed to save integration partition result: {error}");
            if let Err(reset_error) =
                reset_running_job_to_pending(db_pool, job_id, &retry_message).await
            {
                error!(%job_id, %reset_error, "failed to reset integration partition for retry");
            }
            error!(%job_id, %error, "failed to save integration partition result");
            return Err(ProcessJobError::Database);
        }
    } else {
        update_job_result(db_pool, update).await.map_err(|error| {
            error!(%job_id, %error, "failed to save job result in Postgres");
            ProcessJobError::Database
        })?;
    }

    info!(
        %job_id,
        retry_count,
        completed_at = ?completed_at,
        worker_task_id,
        "job processing finished"
    );

    Ok(status)
}

async fn fan_out_integration_job(
    db_pool: &PgPool,
    job_queue: &ActiveJobQueue,
    parent_job: &Job,
) -> Result<JobStatus, ProcessJobError> {
    let input =
        serde_json::from_value::<IntegrationInput>(parent_job.input.clone()).map_err(|error| {
            error!(job_id = %parent_job.id, %error, "invalid integration parent input");
            ProcessJobError::WorkerTask
        })?;
    input.validate().map_err(|error| {
        error!(job_id = %parent_job.id, %error, "invalid integration parent input");
        ProcessJobError::WorkerTask
    })?;
    let sample_partitions =
        partition_samples(input.samples, input.partitions).map_err(|error| {
            error!(job_id = %parent_job.id, %error, "failed to partition integration samples");
            ProcessJobError::WorkerTask
        })?;
    let now = chrono::Utc::now();
    let partitions = sample_partitions
        .into_iter()
        .map(|partition| {
            let partition_input = IntegrationPartitionInput {
                parent_job_id: parent_job.id,
                partition_index: partition.index,
                sample_start: partition.sample_start,
                sample_count: partition.sample_count,
                integration: input.clone(),
            };
            let job = Job {
                id: Uuid::new_v4(),
                task_type: MONTE_CARLO_INTEGRATION_PARTITION_TASK.to_string(),
                status: JobStatus::Pending,
                input: serde_json::to_value(partition_input)
                    .expect("integration partition input should serialize"),
                result: None,
                error: None,
                created_at: now,
                started_at: None,
                completed_at: None,
                retry_count: 0,
            };
            NewJobPartition {
                job,
                parent_job_id: parent_job.id,
                partition_index: partition.index as i32,
            }
        })
        .collect::<Vec<_>>();

    let partition_ids = ensure_job_partitions(db_pool, &partitions)
        .await
        .map_err(|error| {
            error!(job_id = %parent_job.id, %error, "failed to persist integration partitions");
            ProcessJobError::Database
        })?;

    for partition_id in partition_ids {
        if let Err(error) = job_queue.enqueue(partition_id).await {
            let message = format!("failed to enqueue integration partition: {error}");
            if let Err(reset_error) =
                reset_running_job_to_pending(db_pool, parent_job.id, &message).await
            {
                error!(job_id = %parent_job.id, %reset_error, "failed to reset integration parent");
                return Err(ProcessJobError::Database);
            }
            error!(job_id = %parent_job.id, %error, "failed to enqueue integration partition");
            return Err(ProcessJobError::WorkerTask);
        }
    }

    info!(
        job_id = %parent_job.id,
        partitions = input.partitions,
        samples = input.samples,
        "distributed integration partitions enqueued"
    );
    Ok(JobStatus::Running)
}

async fn run_job_blocking(job: Job) -> Result<Result<serde_json::Value, String>, JoinError> {
    tokio::task::spawn_blocking(move || run_job(&job)).await
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
