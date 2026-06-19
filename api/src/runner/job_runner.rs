use crate::jobs::Job;
use crate::tasks::estimate_pi;
use serde_json::{Value, json};

pub fn run_job(job: &Job) -> Result<Value, String> {
    match job.task_type.as_str() {
        "monte_carlo_pi" => run_monte_carlo_pi(job),
        task_type => Err(format!("Unknown task type: {}", task_type)),
    }
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
