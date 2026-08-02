use reqwest::StatusCode;

use crate::{
    api::{JobDetails, format_http_error},
    app::{task_page_count, task_page_range},
    details::detail_lines,
    wizard::{TaskKind, build_request, is_final_field, validate_field},
};

#[test]
fn extracts_json_api_error_message() {
    let message = format_http_error(StatusCode::BAD_REQUEST, r#"{"error":"bad input"}"#);
    assert_eq!(message, "API returned 400 Bad Request: bad input");
}

#[test]
fn builds_pi_request_from_guided_field() {
    let (task, input) = build_request(TaskKind::Pi, &["250000".to_owned()]).expect("valid request");
    assert_eq!(task, "monte_carlo_pi");
    assert_eq!(input["iterations"], 250000);
}

#[test]
fn builds_integration_request_from_guided_fields() {
    let values = [
        "exp(-(x^2 + y^2))",
        "x,y",
        "0,0",
        "1,1",
        "100000",
        "42",
        "8",
    ]
    .map(str::to_owned);
    let (task, input) = build_request(TaskKind::Integration, &values).expect("valid request");
    assert_eq!(task, "monte_carlo_integration");
    assert_eq!(input["variables"], serde_json::json!(["x", "y"]));
    assert_eq!(input["bounds"][1]["max"], 1.0);
    assert_eq!(input["partitions"], 8);
}

#[test]
fn rejects_mismatched_integration_bounds() {
    let previous = ["x".to_owned(), "x,y".to_owned()];
    let error = validate_field(TaskKind::Integration, 2, "0", &previous)
        .expect_err("one bound cannot cover two variables");
    assert_eq!(error, "Enter exactly 2 bound value(s)");
}

#[test]
fn final_wizard_fields_do_not_advance_past_valid_range() {
    assert!(is_final_field(TaskKind::Pi, 0));
    assert!(is_final_field(TaskKind::Integration, 6));
    assert!(!is_final_field(TaskKind::Integration, 5));
}

#[test]
fn task_pages_hold_ten_tasks_and_keep_a_partial_last_page() {
    assert_eq!(task_page_count(0), 1);
    assert_eq!(task_page_count(10), 1);
    assert_eq!(task_page_count(11), 2);
    assert_eq!(task_page_range(0, 23), 0..10);
    assert_eq!(task_page_range(1, 23), 10..20);
    assert_eq!(task_page_range(2, 23), 20..23);
}

#[test]
fn formats_pi_task_details() {
    let job = job_details(serde_json::json!({
        "task_type": "monte_carlo_pi",
        "input": { "iterations": 1000000 },
        "result": { "pi_estimate": 3.14159 }
    }));
    let rendered = detail_lines(&job).join("\n");
    assert!(rendered.contains("Iterations          1000000"));
    assert!(rendered.contains("Pi estimate         3.14159"));
}

#[test]
fn formats_integration_task_details() {
    let job = job_details(serde_json::json!({
        "task_type": "monte_carlo_integration",
        "input": {
            "expression": "x^2",
            "variables": ["x"],
            "bounds": [{ "min": 0.0, "max": 1.0 }],
            "samples": 100000,
            "seed": 42,
            "partitions": 8
        },
        "result": {
            "estimate": 0.333,
            "standard_error": 0.001,
            "confidence_interval_95": { "lower": 0.331, "upper": 0.335 },
            "samples": 100000,
            "seed": 42
        }
    }));
    let rendered = detail_lines(&job).join("\n");
    assert!(rendered.contains("Expression          x^2"));
    assert!(rendered.contains("x            [0.0, 1.0]"));
    assert!(rendered.contains("Estimate            0.333"));
    assert!(rendered.contains("95% confidence       [0.331, 0.335]"));
}

fn job_details(overrides: serde_json::Value) -> JobDetails {
    let mut value = serde_json::json!({
        "id": "f9168ae3-0165-4c4a-b491-c56fd90240c7",
        "task_type": "monte_carlo_pi",
        "status": "COMPLETED",
        "input": {},
        "result": null,
        "error": null,
        "created_at": "2026-08-01T20:00:00Z",
        "started_at": "2026-08-01T20:00:01Z",
        "completed_at": "2026-08-01T20:00:02Z",
        "retry_count": 0,
        "progress": null
    });
    value.as_object_mut().expect("fixture is an object").extend(
        overrides
            .as_object()
            .expect("overrides are an object")
            .clone(),
    );
    serde_json::from_value(value).expect("valid job details fixture")
}
