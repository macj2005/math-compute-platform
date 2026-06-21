use super::{JobQueueBackend, JobQueueConfigError};

#[test]
fn parses_postgres_backend() {
    assert_eq!(
        JobQueueBackend::parse("postgres").expect("backend should parse"),
        JobQueueBackend::Postgres
    );
}

#[test]
fn parses_sqs_backend() {
    assert_eq!(
        JobQueueBackend::parse("sqs").expect("backend should parse"),
        JobQueueBackend::Sqs
    );
}

#[test]
fn rejects_unknown_backend() {
    let error = JobQueueBackend::parse("unknown").expect_err("backend should fail");

    assert!(matches!(error, JobQueueConfigError::UnknownBackend(_)));
}
