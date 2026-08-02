use serde_json::Value;

#[derive(Clone, Copy)]
pub(crate) enum TaskKind {
    Pi,
    Integration,
}

pub(crate) struct RequestWizard {
    pub(crate) kind: Option<TaskKind>,
    pub(crate) field: usize,
    pub(crate) values: Vec<String>,
    pub(crate) input: String,
}

impl RequestWizard {
    pub(crate) fn new() -> Self {
        Self {
            kind: None,
            field: 0,
            values: Vec::new(),
            input: String::new(),
        }
    }

    pub(crate) fn select(&mut self, kind: TaskKind) {
        self.kind = Some(kind);
        self.field = 0;
        self.values.clear();
        self.input.clear();
    }
}

pub(crate) fn task_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Pi => "MONTE CARLO PI",
        TaskKind::Integration => "MONTE CARLO INTEGRATION",
    }
}

pub(crate) fn field_count(kind: TaskKind) -> usize {
    match kind {
        TaskKind::Pi => 1,
        TaskKind::Integration => 7,
    }
}

pub(crate) fn is_final_field(kind: TaskKind, field: usize) -> bool {
    field + 1 == field_count(kind)
}

pub(crate) fn field_label(kind: TaskKind, field: usize) -> &'static str {
    match (kind, field) {
        (TaskKind::Pi, 0) => "Iterations",
        (TaskKind::Integration, 0) => "Expression",
        (TaskKind::Integration, 1) => "Variables",
        (TaskKind::Integration, 2) => "Lower bounds",
        (TaskKind::Integration, 3) => "Upper bounds",
        (TaskKind::Integration, 4) => "Samples",
        (TaskKind::Integration, 5) => "Random seed",
        (TaskKind::Integration, 6) => "Partitions",
        _ => unreachable!("unknown wizard field"),
    }
}

pub(crate) fn field_help(kind: TaskKind, field: usize) -> &'static str {
    match (kind, field) {
        (TaskKind::Pi, 0) => "Number of random points to sample; must be greater than zero.",
        (TaskKind::Integration, 0) => {
            "Expression using variables and sin, cos, tan, exp, ln, sqrt, or abs."
        }
        (TaskKind::Integration, 1) => "Comma-separated variable names. Example: x,y",
        (TaskKind::Integration, 2) => {
            "Comma-separated lower bounds in variable order. Example: 0,0"
        }
        (TaskKind::Integration, 3) => {
            "Comma-separated upper bounds in variable order. Example: 1,1"
        }
        (TaskKind::Integration, 4) => "Total samples, from 2 through 100000000.",
        (TaskKind::Integration, 5) => "Unsigned integer used to make sampling reproducible.",
        (TaskKind::Integration, 6) => "Worker partitions, from 1 through 1024.",
        _ => unreachable!("unknown wizard field"),
    }
}

pub(crate) fn field_default(kind: TaskKind, field: usize) -> Option<&'static str> {
    match (kind, field) {
        (TaskKind::Pi, 0) => Some("1000000"),
        (TaskKind::Integration, 4) => Some("100000"),
        (TaskKind::Integration, 5) => Some("42"),
        (TaskKind::Integration, 6) => Some("8"),
        _ => None,
    }
}

pub(crate) fn validate_field(
    kind: TaskKind,
    field: usize,
    value: &str,
    previous: &[String],
) -> Result<(), String> {
    match (kind, field) {
        (TaskKind::Pi, 0) => {
            if parse_u64(value, "Iterations")? == 0 {
                return Err("Iterations must be greater than zero".to_owned());
            }
        }
        (TaskKind::Integration, 0) if value.len() > 1_000 => {
            return Err("Expression cannot exceed 1000 characters".to_owned());
        }
        (TaskKind::Integration, 0) => {}
        (TaskKind::Integration, 1) => validate_variables(value)?,
        (TaskKind::Integration, 2) | (TaskKind::Integration, 3) => {
            validate_bounds(kind, field, value, previous)?;
        }
        (TaskKind::Integration, 4) => {
            let samples = parse_u64(value, "Samples")?;
            if !(2..=100_000_000).contains(&samples) {
                return Err("Samples must be between 2 and 100000000".to_owned());
            }
        }
        (TaskKind::Integration, 5) => {
            parse_u64(value, "Random seed")?;
        }
        (TaskKind::Integration, 6) => validate_partitions(value, previous)?,
        _ => unreachable!("unknown wizard field"),
    }
    Ok(())
}

pub(crate) fn build_request(
    kind: TaskKind,
    values: &[String],
) -> Result<(&'static str, Value), String> {
    match kind {
        TaskKind::Pi => Ok((
            "monte_carlo_pi",
            serde_json::json!({ "iterations": parse_u64(&values[0], "Iterations")? }),
        )),
        TaskKind::Integration => build_integration_request(values),
    }
}

fn build_integration_request(values: &[String]) -> Result<(&'static str, Value), String> {
    let variables = csv_values(&values[1]);
    let lower = parse_floats(&values[2], "Lower bounds")?;
    let upper = parse_floats(&values[3], "Upper bounds")?;
    let bounds: Vec<Value> = lower
        .into_iter()
        .zip(upper)
        .map(|(min, max)| serde_json::json!({ "min": min, "max": max }))
        .collect();

    Ok((
        "monte_carlo_integration",
        serde_json::json!({
            "expression": values[0],
            "variables": variables,
            "bounds": bounds,
            "samples": parse_u64(&values[4], "Samples")?,
            "seed": parse_u64(&values[5], "Random seed")?,
            "partitions": values[6]
                .parse::<u32>()
                .map_err(|_| "Invalid partitions".to_owned())?,
        }),
    ))
}

fn validate_variables(value: &str) -> Result<(), String> {
    let variables = csv_values(value);
    if variables.is_empty() || variables.len() > 10 {
        return Err("Enter between 1 and 10 variable names".to_owned());
    }
    for variable in &variables {
        if !valid_variable(variable) {
            return Err(format!("Invalid variable name: {variable}"));
        }
    }
    let mut unique = variables.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != variables.len() {
        return Err("Variable names must be unique".to_owned());
    }
    Ok(())
}

fn validate_bounds(
    kind: TaskKind,
    field: usize,
    value: &str,
    previous: &[String],
) -> Result<(), String> {
    let bounds = parse_floats(value, field_label(kind, field))?;
    let variable_count = csv_values(&previous[1]).len();
    if bounds.len() != variable_count {
        return Err(format!("Enter exactly {variable_count} bound value(s)"));
    }
    if field == 3 {
        let lower = parse_floats(&previous[2], "Lower bounds")?;
        if lower.iter().zip(&bounds).any(|(min, max)| min >= max) {
            return Err("Every upper bound must be greater than its lower bound".to_owned());
        }
    }
    Ok(())
}

fn validate_partitions(value: &str, previous: &[String]) -> Result<(), String> {
    let partitions = value
        .parse::<u32>()
        .map_err(|_| "Partitions must be an unsigned integer".to_owned())?;
    if !(1..=1_024).contains(&partitions) {
        return Err("Partitions must be between 1 and 1024".to_owned());
    }
    if u64::from(partitions) > parse_u64(&previous[4], "Samples")? {
        return Err("Partitions cannot exceed samples".to_owned());
    }
    Ok(())
}

fn csv_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_floats(value: &str, label: &str) -> Result<Vec<f64>, String> {
    let values = csv_values(value);
    if values.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    values
        .iter()
        .map(|part| {
            part.parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("{label} must contain finite numbers"))
        })
        .collect()
}

fn parse_u64(value: &str, label: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{label} must be an unsigned integer"))
}

fn valid_variable(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}
