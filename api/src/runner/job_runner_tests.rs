use super::{MONTE_CARLO_INTEGRATION_PARTITION_TASK, MONTE_CARLO_PI_TASK, run_job};
use crate::jobs::{Job, JobStatus};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

#[test]
fn runs_monte_carlo_pi_job() {
    let job = test_job(MONTE_CARLO_PI_TASK, json!({ "iterations": 1000 }));

    let result = run_job(&job).expect("job should run");
    let pi_estimate = result
        .get("pi_estimate")
        .and_then(|value| value.as_f64())
        .expect("result should include pi_estimate");

    assert!(
        (2.0..=4.0).contains(&pi_estimate),
        "expected pi estimate to be plausible, got {pi_estimate}"
    );
}

#[test]
fn rejects_unknown_task_type() {
    let job = test_job("unknown_task", json!({ "iterations": 1000 }));

    let error = run_job(&job).expect_err("job should fail");

    assert_eq!(error, "Unknown task type: unknown_task");
}

#[test]
fn rejects_missing_iterations() {
    let job = test_job(MONTE_CARLO_PI_TASK, json!({}));

    let error = run_job(&job).expect_err("job should fail");

    assert_eq!(error, "iterations must be a u64");
}

#[test]
fn runs_monte_carlo_integration_partition_job() {
    let parent_job_id = Uuid::new_v4();
    let job = test_job(
        MONTE_CARLO_INTEGRATION_PARTITION_TASK,
        json!({
            "parent_job_id": parent_job_id,
            "partition_index": 0,
            "sample_start": 0,
            "sample_count": 50_000,
            "integration": {
                "expression": "x^2",
                "variables": ["x"],
                "bounds": [{ "min": 0.0, "max": 1.0 }],
                "samples": 50_000,
                "seed": 42,
                "partitions": 1
            }
        }),
    );

    let result = run_job(&job).expect("job should run");
    let mean = result["mean"]
        .as_f64()
        .expect("result should include a mean");

    assert!((mean - 1.0 / 3.0).abs() < 0.01);
    assert_eq!(result["sample_count"], 50_000);
}

fn test_job(task_type: &str, input: serde_json::Value) -> Job {
    Job {
        id: Uuid::new_v4(),
        task_type: task_type.to_string(),
        status: JobStatus::Pending,
        input,
        result: None,
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        retry_count: 0,
    }
}
