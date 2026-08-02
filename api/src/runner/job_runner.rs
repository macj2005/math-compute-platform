use crate::jobs::Job;
use crate::tasks::{
    IntegrationPartitionInput, MONTE_CARLO_INTEGRATION_PARTITION_TASK,
    MONTE_CARLO_INTEGRATION_TASK, estimate_pi, sample_partition,
};
use serde_json::{Value, json};

pub const MONTE_CARLO_PI_TASK: &str = "monte_carlo_pi";

pub fn run_job(job: &Job) -> Result<Value, String> {
    match job.task_type.as_str() {
        MONTE_CARLO_PI_TASK => run_monte_carlo_pi(job),
        MONTE_CARLO_INTEGRATION_TASK => {
            Err("monte_carlo_integration is a coordinator task and must be partitioned".to_string())
        }
        MONTE_CARLO_INTEGRATION_PARTITION_TASK => run_monte_carlo_integration_partition(job),
        task_type => Err(format!("Unknown task type: {}", task_type)),
    }
}

fn run_monte_carlo_integration_partition(job: &Job) -> Result<Value, String> {
    let input = serde_json::from_value::<IntegrationPartitionInput>(job.input.clone())
        .map_err(|error| format!("invalid integration partition input: {error}"))?;
    let result = sample_partition(&input.integration, input.sample_start, input.sample_count)?;

    serde_json::to_value(result)
        .map_err(|error| format!("failed to serialize integration partition result: {error}"))
}

fn run_monte_carlo_pi(job: &Job) -> Result<Value, String> {
    let iterations = job
        .input
        .get("iterations")
        .and_then(Value::as_u64)
        .ok_or_else(|| "iterations must be a u64".to_string())?;

    let pi_estimate = estimate_pi(iterations);

    Ok(json!({ "pi_estimate": pi_estimate }))
}

#[cfg(test)]
#[path = "job_runner_tests.rs"]
mod tests;
