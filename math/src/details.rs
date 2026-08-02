use serde_json::Value;

use crate::api::{JobDetails, JobProgress};

pub(crate) fn detail_lines(job: &JobDetails) -> Vec<String> {
    let mut lines = vec![
        "TASK DETAILS".to_owned(),
        String::new(),
        format!("ID          {}", job.id),
        format!("Type        {}", display_task_type(&job.task_type)),
        format!("Status      {}", job.status),
        format!("Retries     {}", job.retry_count),
        format!("Created     {}", job.created_at),
        format!("Started     {}", optional_text(job.started_at.as_deref())),
        format!("Completed   {}", optional_text(job.completed_at.as_deref())),
    ];

    if let Some(progress) = &job.progress {
        lines.extend(progress_lines(progress));
    }

    lines.push(String::new());
    lines.push("INPUT".to_owned());
    lines.extend(input_lines(job));
    lines.push(String::new());
    lines.push("RESULT".to_owned());
    lines.extend(result_lines(job));
    lines.push(String::new());
    lines.push("Press Esc to return to the task list.".to_owned());
    lines
}

fn display_task_type(task_type: &str) -> &str {
    match task_type {
        "monte_carlo_pi" => "Monte Carlo Pi Estimation",
        "monte_carlo_integration" => "Monte Carlo Integration",
        _ => task_type,
    }
}

fn input_lines(job: &JobDetails) -> Vec<String> {
    match job.task_type.as_str() {
        "monte_carlo_pi" => vec![format!(
            "Iterations          {}",
            number(&job.input, "iterations")
        )],
        "monte_carlo_integration" => integration_input_lines(&job.input),
        _ => pretty_json(&job.input),
    }
}

fn integration_input_lines(input: &Value) -> Vec<String> {
    let mut lines = vec![
        format!("Expression          {}", text(input, "expression")),
        format!("Samples             {}", number(input, "samples")),
        format!("Random seed         {}", number(input, "seed")),
        format!("Partitions          {}", number(input, "partitions")),
    ];

    let variables = input.get("variables").and_then(Value::as_array);
    let bounds = input.get("bounds").and_then(Value::as_array);
    lines.push("Domain".to_owned());
    match (variables, bounds) {
        (Some(variables), Some(bounds)) => {
            for (variable, bound) in variables.iter().zip(bounds) {
                lines.push(format!(
                    "  {:<12} [{}, {}]",
                    variable.as_str().unwrap_or("?"),
                    number(bound, "min"),
                    number(bound, "max")
                ));
            }
        }
        _ => lines.push("  Not available".to_owned()),
    }
    lines
}

fn result_lines(job: &JobDetails) -> Vec<String> {
    if let Some(error) = &job.error {
        return vec![format!("Error               {error}")];
    }
    let Some(result) = &job.result else {
        return vec![format!(
            "No result yet; task is {}.",
            job.status.to_lowercase()
        )];
    };

    match job.task_type.as_str() {
        "monte_carlo_pi" => vec![format!(
            "Pi estimate         {}",
            number(result, "pi_estimate")
        )],
        "monte_carlo_integration" => integration_result_lines(result),
        _ => pretty_json(result),
    }
}

fn integration_result_lines(result: &Value) -> Vec<String> {
    let interval = result.get("confidence_interval_95");
    vec![
        format!("Estimate            {}", number(result, "estimate")),
        format!("Standard error       {}", number(result, "standard_error")),
        format!(
            "95% confidence       [{}, {}]",
            interval
                .map(|value| number(value, "lower"))
                .unwrap_or_else(|| "?".to_owned()),
            interval
                .map(|value| number(value, "upper"))
                .unwrap_or_else(|| "?".to_owned())
        ),
        format!("Samples             {}", number(result, "samples")),
        format!("Random seed         {}", number(result, "seed")),
    ]
}

fn progress_lines(progress: &JobProgress) -> Vec<String> {
    vec![
        String::new(),
        "PROGRESS".to_owned(),
        format!("Complete            {:.1}%", progress.percent),
        format!(
            "Partitions          {} total | {} pending | {} running | {} complete | {} failed",
            progress.total_partitions,
            progress.pending_partitions,
            progress.running_partitions,
            progress.completed_partitions,
            progress.failed_partitions
        ),
        format!(
            "Samples             {} / {}",
            progress.completed_samples, progress.total_samples
        ),
    ]
}

fn number(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(Value::to_string)
        .unwrap_or_else(|| "?".to_owned())
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("?")
}

fn optional_text(value: Option<&str>) -> &str {
    value.unwrap_or("Not yet")
}

fn pretty_json(value: &Value) -> Vec<String> {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| value.to_string())
        .lines()
        .map(str::to_owned)
        .collect()
}
