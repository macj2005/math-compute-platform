use chrono::Utc;
use tokio::time::{Duration, sleep};
use tracing::{error, info};
use uuid::Uuid;

use crate::jobs::{Job, JobStatus, JobStore};
use crate::runner::run_job;

#[derive(Debug)]
pub enum ProcessJobError {
    NotFound,
    NotPending,
}

pub async fn start_worker_loop(job_store: JobStore) {
    info!("background worker loop started");

    loop {
        if let Some(job_id) = process_next_pending_job(job_store.clone()) {
            info!(%job_id, "processed pending job - sleeping 1 second");
        }

        sleep(Duration::from_secs(1)).await;
    }
}

pub fn process_next_pending_job(job_store: JobStore) -> Option<Uuid> {
    let job_to_run = claim_next_pending_job(job_store.clone())?;
    let job_id = job_to_run.id;

    save_job_result(job_store, job_to_run);

    Some(job_id)
}

pub fn process_job_by_id(job_store: JobStore, job_id: Uuid) -> Result<Job, ProcessJobError> {
    let job_to_run = claim_job_by_id(job_store.clone(), job_id)?;

    save_job_result(job_store.clone(), job_to_run);

    let jobs = job_store.lock().expect("job store lock poisoned");
    jobs.get(&job_id).cloned().ok_or(ProcessJobError::NotFound)
}

fn claim_next_pending_job(job_store: JobStore) -> Option<Job> {
    let mut jobs = job_store.lock().expect("job store lock poisoned");
    let job = jobs
        .values_mut()
        .find(|job| job.status == JobStatus::Pending)?;

    job.status = JobStatus::Running;
    job.started_at = Some(Utc::now());
    job.error = None;

    Some(job.clone())
}

fn claim_job_by_id(job_store: JobStore, job_id: Uuid) -> Result<Job, ProcessJobError> {
    let mut jobs = job_store.lock().expect("job store lock poisoned");
    let job = jobs.get_mut(&job_id).ok_or(ProcessJobError::NotFound)?;

    if job.status != JobStatus::Pending {
        return Err(ProcessJobError::NotPending);
    }

    job.status = JobStatus::Running;
    job.started_at = Some(Utc::now());
    job.error = None;

    Ok(job.clone())
}

fn save_job_result(job_store: JobStore, job_to_run: Job) {
    let job_id = job_to_run.id;
    let result = run_job(&job_to_run);

    let mut jobs = job_store.lock().expect("job store lock poisoned");
    let Some(job) = jobs.get_mut(&job_id) else {
        error!(%job_id, "claimed job disappeared before result could be saved");
        return;
    };

    match result {
        Ok(output) => {
            job.status = JobStatus::Completed;
            job.result = Some(output.clone());
            job.error = None;

            info!(
                job_id = %job.id,
                task_type = job.task_type.as_str(),
                result = %output,
                "job completed successfully"
            );
        }
        Err(error) => {
            job.status = JobStatus::Failed;
            job.result = None;
            job.error = Some(error.clone());

            error!(
                job_id = %job.id,
                task_type = job.task_type.as_str(),
                error = error.as_str(),
                "job failed"
            );
        }
    }

    job.completed_at = Some(Utc::now());

    info!(
        job_id = %job.id,
        status = ?job.status,
        completed_at = ?job.completed_at,
        "job processing finished"
    );
}
