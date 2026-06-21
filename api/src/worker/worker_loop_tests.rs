use super::{WorkerConfig, parse_i32_env_value, parse_u64_env_value, parse_usize_env_value};
use std::time::Duration;

#[test]
fn default_config_uses_expected_values() {
    let config = WorkerConfig::default();

    assert_eq!(config.max_retries, 3);
    assert_eq!(config.poll_interval, Duration::from_secs(1));
    assert_eq!(config.concurrency, 1);
}

#[test]
fn parses_valid_config_values() {
    assert_eq!(parse_i32_env_value(Some("5"), 3), 5);
    assert_eq!(parse_u64_env_value(Some("10"), 1), 10);
    assert_eq!(parse_usize_env_value(Some("4"), 1), 4);
}

#[test]
fn falls_back_to_defaults_for_invalid_config_values() {
    assert_eq!(parse_i32_env_value(Some("-1"), 3), 3);
    assert_eq!(parse_i32_env_value(Some("not-a-number"), 3), 3);
    assert_eq!(parse_u64_env_value(Some("0"), 1), 1);
    assert_eq!(parse_u64_env_value(None, 1), 1);
    assert_eq!(parse_usize_env_value(Some("0"), 1), 1);
    assert_eq!(parse_usize_env_value(None, 1), 1);
}
