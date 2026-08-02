use std::collections::HashSet;

use evalexpr::{
    ContextWithMutableFunctions, ContextWithMutableVariables, DefaultNumericTypes, Function,
    HashMapContext, Value, build_operator_tree,
};
use serde::{Deserialize, Serialize};

const MAX_DIMENSIONS: usize = 10;
const MAX_SAMPLES: u64 = 100_000_000;
const MAX_EXPRESSION_LENGTH: usize = 1_000;
const MAX_PARTITIONS: u32 = 1_024;
const DEFAULT_PARTITIONS: u32 = 8;

pub const MONTE_CARLO_INTEGRATION_TASK: &str = "monte_carlo_integration";
pub const MONTE_CARLO_INTEGRATION_PARTITION_TASK: &str = "monte_carlo_integration_partition";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Bounds {
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct IntegrationInput {
    pub expression: String,
    pub variables: Vec<String>,
    pub bounds: Vec<Bounds>,
    pub samples: u64,
    pub seed: u64,
    #[serde(default = "default_partitions")]
    pub partitions: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct IntegrationPartitionInput {
    pub parent_job_id: uuid::Uuid,
    pub partition_index: u32,
    pub sample_start: u64,
    pub sample_count: u64,
    pub integration: IntegrationInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplePartition {
    pub index: u32,
    pub sample_start: u64,
    pub sample_count: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PartialStatistics {
    pub sample_count: u64,
    pub mean: f64,
    pub squared_deviation_sum: f64,
}

impl PartialStatistics {
    pub fn empty() -> Self {
        Self {
            sample_count: 0,
            mean: 0.0,
            squared_deviation_sum: 0.0,
        }
    }

    pub fn merge(self, other: Self) -> Self {
        if self.sample_count == 0 {
            return other;
        }
        if other.sample_count == 0 {
            return self;
        }

        let left_count = self.sample_count as f64;
        let right_count = other.sample_count as f64;
        let combined_count = left_count + right_count;
        let mean_delta = other.mean - self.mean;

        Self {
            sample_count: self.sample_count + other.sample_count,
            mean: self.mean + mean_delta * right_count / combined_count,
            squared_deviation_sum: self.squared_deviation_sum
                + other.squared_deviation_sum
                + mean_delta * mean_delta * left_count * right_count / combined_count,
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ConfidenceInterval {
    pub lower: f64,
    pub upper: f64,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct IntegrationResult {
    pub estimate: f64,
    pub standard_error: f64,
    pub confidence_interval_95: ConfidenceInterval,
    pub samples: u64,
    pub seed: u64,
}

impl IntegrationInput {
    pub fn validate(&self) -> Result<(), String> {
        let expression = self.expression.trim();
        if expression.is_empty() {
            return Err("expression must not be empty".to_string());
        }
        if expression.len() > MAX_EXPRESSION_LENGTH {
            return Err(format!(
                "expression must not exceed {MAX_EXPRESSION_LENGTH} characters"
            ));
        }
        if self.samples < 2 {
            return Err("samples must be at least 2".to_string());
        }
        if self.samples > MAX_SAMPLES {
            return Err(format!("samples must not exceed {MAX_SAMPLES}"));
        }
        if self.partitions == 0 {
            return Err("partitions must be greater than 0".to_string());
        }
        if self.partitions > MAX_PARTITIONS {
            return Err(format!("partitions must not exceed {MAX_PARTITIONS}"));
        }
        if u64::from(self.partitions) > self.samples {
            return Err("partitions must not exceed samples".to_string());
        }
        if self.variables.is_empty() {
            return Err("at least one variable is required".to_string());
        }
        if self.variables.len() > MAX_DIMENSIONS {
            return Err(format!("dimensions must not exceed {MAX_DIMENSIONS}"));
        }
        if self.variables.len() != self.bounds.len() {
            return Err("each variable must have exactly one set of bounds".to_string());
        }

        let mut unique_variables = HashSet::with_capacity(self.variables.len());
        for variable in &self.variables {
            if !is_valid_variable_name(variable) {
                return Err(format!("invalid variable name: {variable}"));
            }
            if !unique_variables.insert(variable.as_str()) {
                return Err(format!("duplicate variable name: {variable}"));
            }
        }

        for bounds in &self.bounds {
            if !bounds.min.is_finite() || !bounds.max.is_finite() {
                return Err("bounds must be finite".to_string());
            }
            if bounds.min >= bounds.max {
                return Err("each lower bound must be less than its upper bound".to_string());
            }
        }

        parse_expression(expression)?;

        Ok(())
    }
}

pub fn integrate(input: &IntegrationInput) -> Result<IntegrationResult, String> {
    input.validate()?;

    let partial = sample_partition(input, 0, input.samples)?;
    finalize_result(input, partial)
}

pub fn sample_partition(
    input: &IntegrationInput,
    sample_start: u64,
    sample_count: u64,
) -> Result<PartialStatistics, String> {
    input.validate()?;
    if sample_count == 0 {
        return Err("partition sample count must be greater than 0".to_string());
    }
    let sample_end = sample_start
        .checked_add(sample_count)
        .ok_or_else(|| "partition sample range overflowed".to_string())?;
    if sample_end > input.samples {
        return Err("partition sample range exceeds total samples".to_string());
    }

    let expression = parse_expression(&input.expression)?;
    let mut context = math_context()?;

    // Welford's online algorithm avoids the cancellation error in sum(x^2) - sum(x)^2.
    let mut mean = 0.0;
    let mut squared_deviation_sum = 0.0;

    for local_index in 0..sample_count {
        let sample_index = sample_start + local_index;
        for (dimension_index, (variable, bounds)) in
            input.variables.iter().zip(&input.bounds).enumerate()
        {
            let unit_sample = deterministic_unit_sample(input.seed, sample_index, dimension_index);
            let sample = bounds.min + unit_sample * (bounds.max - bounds.min);
            context
                .set_value(variable.clone(), Value::from_float(sample))
                .map_err(|error| format!("failed to bind variable {variable}: {error}"))?;
        }

        let value = expression
            .eval_number_with_context(&context)
            .map_err(|error| format!("failed to evaluate sample {sample_index}: {error}"))?;
        if !value.is_finite() {
            return Err(format!(
                "expression produced a non-finite value at sample {sample_index}"
            ));
        }

        let count = (local_index + 1) as f64;
        let delta = value - mean;
        mean += delta / count;
        let delta_from_new_mean = value - mean;
        squared_deviation_sum += delta * delta_from_new_mean;
    }

    Ok(PartialStatistics {
        sample_count,
        mean,
        squared_deviation_sum,
    })
}

pub fn finalize_result(
    input: &IntegrationInput,
    statistics: PartialStatistics,
) -> Result<IntegrationResult, String> {
    if statistics.sample_count != input.samples {
        return Err(format!(
            "partial statistics contain {} samples; expected {}",
            statistics.sample_count, input.samples
        ));
    }

    let volume = input
        .bounds
        .iter()
        .map(|bounds| bounds.max - bounds.min)
        .product::<f64>();
    let sample_variance = statistics.squared_deviation_sum / (statistics.sample_count - 1) as f64;
    let estimate = volume * statistics.mean;
    let standard_error =
        volume * sample_variance.max(0.0).sqrt() / (statistics.sample_count as f64).sqrt();
    let margin = 1.96 * standard_error;

    Ok(IntegrationResult {
        estimate,
        standard_error,
        confidence_interval_95: ConfidenceInterval {
            lower: estimate - margin,
            upper: estimate + margin,
        },
        samples: input.samples,
        seed: input.seed,
    })
}

pub fn partition_samples(samples: u64, partitions: u32) -> Result<Vec<SamplePartition>, String> {
    if partitions == 0 {
        return Err("partitions must be greater than 0".to_string());
    }
    if u64::from(partitions) > samples {
        return Err("partitions must not exceed samples".to_string());
    }

    let base_size = samples / u64::from(partitions);
    let larger_partition_count = samples % u64::from(partitions);
    let mut sample_start = 0;
    let mut ranges = Vec::with_capacity(partitions as usize);

    for index in 0..partitions {
        let sample_count = base_size + u64::from(u64::from(index) < larger_partition_count);
        ranges.push(SamplePartition {
            index,
            sample_start,
            sample_count,
        });
        sample_start += sample_count;
    }

    Ok(ranges)
}

fn deterministic_unit_sample(seed: u64, sample_index: u64, dimension_index: usize) -> f64 {
    let mixed_input = seed
        ^ sample_index.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (dimension_index as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
    let random_bits = splitmix64(mixed_input);

    ((random_bits >> 11) as f64) * (1.0 / ((1_u64 << 53) as f64))
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn default_partitions() -> u32 {
    DEFAULT_PARTITIONS
}

fn parse_expression(expression: &str) -> Result<evalexpr::Node<DefaultNumericTypes>, String> {
    build_operator_tree::<DefaultNumericTypes>(expression)
        .map_err(|error| format!("invalid expression: {error}"))
}

fn math_context() -> Result<HashMapContext<DefaultNumericTypes>, String> {
    let mut context = HashMapContext::<DefaultNumericTypes>::new();
    add_unary_function(&mut context, "sin", f64::sin)?;
    add_unary_function(&mut context, "cos", f64::cos)?;
    add_unary_function(&mut context, "tan", f64::tan)?;
    add_unary_function(&mut context, "exp", f64::exp)?;
    add_unary_function(&mut context, "ln", f64::ln)?;
    add_unary_function(&mut context, "sqrt", f64::sqrt)?;
    add_unary_function(&mut context, "abs", f64::abs)?;
    Ok(context)
}

fn add_unary_function(
    context: &mut HashMapContext<DefaultNumericTypes>,
    name: &str,
    operation: fn(f64) -> f64,
) -> Result<(), String> {
    context
        .set_function(
            name.to_string(),
            Function::new(move |argument| Ok(Value::from_float(operation(argument.as_number()?)))),
        )
        .map_err(|error| format!("failed to register function {name}: {error}"))
}

fn is_valid_variable_name(variable: &str) -> bool {
    let mut characters = variable.chars();
    let Some(first) = characters.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
#[path = "monte_carlo_integration_tests.rs"]
mod tests;
