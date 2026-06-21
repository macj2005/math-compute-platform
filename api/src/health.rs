mod health_endpoints;

pub use health_endpoints::{health_check, ready_check};

#[cfg(test)]
mod health_tests;
