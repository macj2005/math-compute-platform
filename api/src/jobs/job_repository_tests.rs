use super::{
    JobResultUpdate, claim_job_by_id_from_db, claim_next_pending_job_from_db, get_job_by_id,
    insert_job, list_jobs_from_db, update_job_result,
};
use crate::jobs::{Job, JobStatus};
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

#[tokio::test]
async fn inserts_gets_and_lists_jobs() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };

    let job = test_job(Uuid::new_v4(), Utc::now());
    cleanup_jobs(&db_pool, &[job.id]).await;

    insert_job(&db_pool, &job)
        .await
        .expect("insert should work");

    let saved_job = get_job_by_id(&db_pool, job.id)
        .await
        .expect("get should work")
        .expect("job should exist");
    let listed_jobs = list_jobs_from_db(&db_pool).await.expect("list should work");

    assert_eq!(saved_job.id, job.id);
    assert_eq!(saved_job.task_type, job.task_type);
    assert_eq!(saved_job.status, JobStatus::Pending);
    assert_eq!(saved_job.input, job.input);
    assert!(listed_jobs.iter().any(|listed_job| listed_job.id == job.id));

    cleanup_jobs(&db_pool, &[job.id]).await;
}

#[tokio::test]
async fn claims_next_pending_job_as_running() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };

    let job = test_job(Uuid::new_v4(), Utc::now() - Duration::days(20_000));
    cleanup_jobs(&db_pool, &[job.id]).await;
    insert_job(&db_pool, &job)
        .await
        .expect("insert should work");

    let claimed_job = claim_next_pending_job_from_db(&db_pool)
        .await
        .expect("claim should work")
        .expect("job should be claimed");

    assert_eq!(claimed_job.id, job.id);
    assert_eq!(claimed_job.status, JobStatus::Running);
    assert!(claimed_job.started_at.is_some());

    cleanup_jobs(&db_pool, &[job.id]).await;
}

#[tokio::test]
async fn claims_specific_pending_job_by_id() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };

    let job = test_job(Uuid::new_v4(), Utc::now());
    cleanup_jobs(&db_pool, &[job.id]).await;
    insert_job(&db_pool, &job)
        .await
        .expect("insert should work");

    let claimed_job = claim_job_by_id_from_db(&db_pool, job.id)
        .await
        .expect("claim by id should work")
        .expect("job should be claimed");
    let second_claim = claim_job_by_id_from_db(&db_pool, job.id)
        .await
        .expect("second claim should work");

    assert_eq!(claimed_job.id, job.id);
    assert_eq!(claimed_job.status, JobStatus::Running);
    assert!(second_claim.is_none());

    cleanup_jobs(&db_pool, &[job.id]).await;
}

#[tokio::test]
async fn updates_job_result() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };

    let job = test_job(Uuid::new_v4(), Utc::now());
    cleanup_jobs(&db_pool, &[job.id]).await;
    insert_job(&db_pool, &job)
        .await
        .expect("insert should work");

    let completed_at = Utc::now();
    update_job_result(
        &db_pool,
        JobResultUpdate {
            id: job.id,
            status: JobStatus::Completed,
            result: Some(json!({ "pi_estimate": 3.14 })),
            error: None,
            completed_at: Some(completed_at),
            retry_count: 1,
        },
    )
    .await
    .expect("update should work");

    let saved_job = get_job_by_id(&db_pool, job.id)
        .await
        .expect("get should work")
        .expect("job should exist");

    assert_eq!(saved_job.status, JobStatus::Completed);
    assert_eq!(saved_job.result, Some(json!({ "pi_estimate": 3.14 })));
    assert_eq!(saved_job.error, None);
    assert_eq!(saved_job.retry_count, 1);
    assert!(saved_job.completed_at.is_some());

    cleanup_jobs(&db_pool, &[job.id]).await;
}

#[tokio::test]
async fn concurrent_claims_receive_different_jobs() {
    let _guard = crate::test_support::db_test_guard().await;
    let Some(db_pool) = test_db_pool().await else {
        return;
    };

    let first_job = test_job(Uuid::new_v4(), Utc::now() - Duration::days(20_001));
    let second_job = test_job(Uuid::new_v4(), Utc::now() - Duration::days(20_000));
    let job_ids = [first_job.id, second_job.id];
    cleanup_jobs(&db_pool, &job_ids).await;

    insert_job(&db_pool, &first_job)
        .await
        .expect("first insert should work");
    insert_job(&db_pool, &second_job)
        .await
        .expect("second insert should work");

    let (first_claim, second_claim) = tokio::join!(
        claim_next_pending_job_from_db(&db_pool),
        claim_next_pending_job_from_db(&db_pool),
    );

    let first_claim = first_claim
        .expect("first claim should work")
        .expect("first claim should find a job");
    let second_claim = second_claim
        .expect("second claim should work")
        .expect("second claim should find a job");

    assert_ne!(first_claim.id, second_claim.id);
    assert!(job_ids.contains(&first_claim.id));
    assert!(job_ids.contains(&second_claim.id));
    assert_eq!(first_claim.status, JobStatus::Running);
    assert_eq!(second_claim.status, JobStatus::Running);

    cleanup_jobs(&db_pool, &job_ids).await;
}

async fn test_db_pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect to test Postgres");

    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("failed to run test database migrations");

    Some(db_pool)
}

fn test_job(id: Uuid, created_at: chrono::DateTime<Utc>) -> Job {
    Job {
        id,
        task_type: "monte_carlo_pi".to_string(),
        status: JobStatus::Pending,
        input: json!({ "iterations": 1000 }),
        result: None,
        error: None,
        created_at,
        started_at: None,
        completed_at: None,
        retry_count: 0,
    }
}

async fn cleanup_jobs(db_pool: &PgPool, job_ids: &[Uuid]) {
    for job_id in job_ids {
        sqlx::query("DELETE FROM jobs WHERE id = $1")
            .bind(job_id)
            .execute(db_pool)
            .await
            .expect("test cleanup should work");
    }
}
