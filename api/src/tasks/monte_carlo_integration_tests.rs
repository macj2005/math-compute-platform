use super::{
    Bounds, IntegrationInput, PartialStatistics, integrate, partition_samples, sample_partition,
};

fn valid_input() -> IntegrationInput {
    IntegrationInput {
        expression: "exp(-(x^2 + y^2))".to_string(),
        variables: vec!["x".to_string(), "y".to_string()],
        bounds: vec![Bounds { min: 0.0, max: 1.0 }, Bounds { min: 0.0, max: 1.0 }],
        samples: 10_000,
        seed: 42,
        partitions: 8,
    }
}

#[test]
fn accepts_valid_multidimensional_input() {
    assert_eq!(valid_input().validate(), Ok(()));
}

#[test]
fn rejects_too_few_samples() {
    let mut input = valid_input();
    input.samples = 1;

    assert_eq!(
        input.validate(),
        Err("samples must be at least 2".to_string())
    );
}

#[test]
fn rejects_variable_and_bounds_mismatch() {
    let mut input = valid_input();
    input.bounds.pop();

    assert_eq!(
        input.validate(),
        Err("each variable must have exactly one set of bounds".to_string())
    );
}

#[test]
fn rejects_duplicate_variables() {
    let mut input = valid_input();
    input.variables[1] = "x".to_string();

    assert_eq!(
        input.validate(),
        Err("duplicate variable name: x".to_string())
    );
}

#[test]
fn rejects_invalid_variable_names() {
    let mut input = valid_input();
    input.variables[0] = "2x".to_string();

    assert_eq!(
        input.validate(),
        Err("invalid variable name: 2x".to_string())
    );
}

#[test]
fn rejects_non_finite_or_reversed_bounds() {
    let mut non_finite = valid_input();
    non_finite.bounds[0].max = f64::INFINITY;
    assert_eq!(
        non_finite.validate(),
        Err("bounds must be finite".to_string())
    );

    let mut reversed = valid_input();
    reversed.bounds[0] = Bounds { min: 1.0, max: 0.0 };
    assert_eq!(
        reversed.validate(),
        Err("each lower bound must be less than its upper bound".to_string())
    );
}

#[test]
fn integrates_x_squared() {
    let mut input = valid_input();
    input.expression = "x^2".to_string();
    input.variables = vec!["x".to_string()];
    input.bounds = vec![Bounds { min: 0.0, max: 1.0 }];
    input.samples = 50_000;

    let result = integrate(&input).expect("integration should succeed");

    assert!((result.estimate - 1.0 / 3.0).abs() < 0.01);
    assert!(result.standard_error > 0.0);
    assert!(result.confidence_interval_95.lower < result.estimate);
    assert!(result.confidence_interval_95.upper > result.estimate);
}

#[test]
fn integrates_sine() {
    let mut input = valid_input();
    input.expression = "sin(x)".to_string();
    input.variables = vec!["x".to_string()];
    input.bounds = vec![Bounds {
        min: 0.0,
        max: std::f64::consts::PI,
    }];
    input.samples = 50_000;

    let result = integrate(&input).expect("integration should succeed");

    assert!((result.estimate - 2.0).abs() < 0.02);
}

#[test]
fn produces_identical_results_for_the_same_seed() {
    let input = valid_input();

    assert_eq!(
        integrate(&input).expect("first integration should succeed"),
        integrate(&input).expect("second integration should succeed")
    );
}

#[test]
fn rejects_invalid_expressions() {
    let mut input = valid_input();
    input.expression = "x + (".to_string();

    let error = input.validate().expect_err("expression should be rejected");

    assert!(error.starts_with("invalid expression:"));
}

#[test]
fn rejects_non_finite_results() {
    let mut input = valid_input();
    input.expression = "1 / (x - x)".to_string();
    input.variables = vec!["x".to_string()];
    input.bounds = vec![Bounds { min: 0.0, max: 1.0 }];

    let error = integrate(&input).expect_err("non-finite result should be rejected");

    assert!(error.starts_with("expression produced a non-finite value at sample"));
}

#[test]
fn partitions_cover_every_sample_exactly_once() {
    let ranges = partition_samples(10, 3).expect("partitioning should succeed");

    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0].sample_start, 0);
    assert_eq!(ranges[0].sample_count, 4);
    assert_eq!(ranges[1].sample_start, 4);
    assert_eq!(ranges[1].sample_count, 3);
    assert_eq!(ranges[2].sample_start, 7);
    assert_eq!(ranges[2].sample_count, 3);
    assert_eq!(
        ranges.iter().map(|range| range.sample_count).sum::<u64>(),
        10
    );
}

#[test]
fn merged_partitions_match_a_single_partition() {
    let input = valid_input();
    let full = sample_partition(&input, 0, input.samples).expect("full sample should work");
    let merged = partition_samples(input.samples, input.partitions)
        .expect("partitioning should work")
        .into_iter()
        .map(|partition| {
            sample_partition(&input, partition.sample_start, partition.sample_count)
                .expect("partition should run")
        })
        .fold(PartialStatistics::empty(), PartialStatistics::merge);

    assert_eq!(merged.sample_count, full.sample_count);
    assert!((merged.mean - full.mean).abs() < 1e-12);
    assert!((merged.squared_deviation_sum - full.squared_deviation_sum).abs() < 1e-9);
}

#[test]
fn retried_partition_produces_identical_statistics() {
    let input = valid_input();

    assert_eq!(
        sample_partition(&input, 500, 1_000).expect("first attempt should work"),
        sample_partition(&input, 500, 1_000).expect("retry should work")
    );
}
