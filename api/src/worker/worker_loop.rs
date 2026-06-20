use sqlx::PgPool;
use tokio::time::{Duration, sleep};
use tracing::{error, info};
use uuid::Uuid;

use crate::jobs::{
    Job, JobResultUpdate, JobStatus, claim_job_by_id_from_db, claim_next_pending_job_from_db,
    get_job_by_id, update_job_result,
};
use crate::runner::run_job;

#[derive(Debug)]
pub enum ProcessJobError {
    NotFound,
    NotPending,
    Database,
}

pub async fn start_worker_loop(db_pool: PgPool) {
    info!("background worker loop started");

    loop {
        if let Some(job_id) = process_next_pending_job(db_pool.clone()).await {
            info!(%job_id, "processed pending job - sleeping 1 second");
        }

        sleep(Duration::from_secs(1)).await;
    }
}

pub async fn process_next_pending_job(db_pool: PgPool) -> Option<Uuid> {
    let job_to_run = match claim_next_pending_job_from_db(&db_pool).await {
        Ok(Some(job)) => job,
        Ok(None) => return None,
        Err(error) => {
            error!(%error, "failed to claim pending job from Postgres");
            return None;
        }
    };

    let job_id = job_to_run.id;

    if let Err(error) = save_job_result(&db_pool, job_to_run).await {
        error!(%job_id, ?error, "failed to finish pending job");
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

    save_job_result(&db_pool, job_to_run).await?;

    get_job_by_id(&db_pool, job_id)
        .await
        .map_err(|error| {
            error!(%job_id, %error, "failed to load completed job from Postgres");
            ProcessJobError::Database
        })?
        .ok_or(ProcessJobError::NotFound)
}

async fn save_job_result(db_pool: &PgPool, job_to_run: Job) -> Result<(), ProcessJobError> {
    let job_id = job_to_run.id;
    let result = run_job(&job_to_run);

    let (status, output, error_message) = match result {
        Ok(output) => {
            info!(
                job_id = %job_to_run.id,
                task_type = job_to_run.task_type.as_str(),
                result = %output,
                "job completed successfully"
            );

            (JobStatus::Completed, Some(output), None)
        }
        Err(error) => {
            error!(
                job_id = %job_to_run.id,
                task_type = job_to_run.task_type.as_str(),
                error = error.as_str(),
                "job failed"
            );

            (JobStatus::Failed, None, Some(error))
        }
    };

    let completed_at = chrono::Utc::now();
    let update = JobResultUpdate {
        id: job_to_run.id,
        status,
        result: output,
        error: error_message,
        completed_at: Some(completed_at),
    };

    update_job_result(db_pool, update).await.map_err(|error| {
        error!(%job_id, %error, "failed to save job result in Postgres");
        ProcessJobError::Database
    })?;

    info!(
        job_id = %job_to_run.id,
        completed_at = ?completed_at,
        "job processing finished"
    );

    Ok(())
}
