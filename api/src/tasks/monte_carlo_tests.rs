use super::estimate_pi;

#[test]
fn returns_zero_for_zero_iterations() {
    assert_eq!(estimate_pi(0), 0.0);
}

#[test]
fn estimates_pi_within_reasonable_range() {
    let pi_estimate = estimate_pi(10_000);

    assert!(
        (2.5..=3.8).contains(&pi_estimate),
        "expected pi estimate to be plausible, got {pi_estimate}"
    );
}
