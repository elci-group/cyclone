//! Typed current intermediate representation.
//!
//! `cyclone.cir/v0` is an interpret-only lookup table. Validation is the type
//! check. A value that fails it is not a current.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CIR_V0: &str = "cyclone.cir/v0";
pub const SCHEDULE_INTERPRET: &str = "interpret";
pub const FALLBACK_AGENT: &str = "original_agent_workflow";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IrError {
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentIr {
    pub schema: String,
    pub current_id: String,
    pub version: u32,
    pub tenant_id: String,
    pub signature: Vec<String>,
    pub pressure: String,
    pub steps: Vec<Step>,
    pub table: Vec<IoRow>,
    pub precondition: Precondition,
    pub postcondition: Postcondition,
    pub environment: Environment,
    pub side_effects: Vec<String>,
    pub retry: Retry,
    pub idempotency: String,
    pub schedule: String,
    pub fallback: String,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub action_kind: String,
    pub effect: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoRow {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Precondition {
    pub environment_eq: String,
    pub input_in_table: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Postcondition {
    pub output_from_table: bool,
    pub equal_digest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retry {
    pub max_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub agent_executions: u64,
    pub agreement_matching: u64,
    pub agreement_considered: u64,
    pub economics_minor: i64,
    pub observation_ids: Vec<String>,
}

pub fn validate(ir: &CurrentIr) -> Result<(), IrError> {
    if ir.schema != CIR_V0 {
        return invalid("schema must be cyclone.cir/v0");
    }
    if ir.version != 1 {
        return invalid("version must be 1");
    }
    if ir.tenant_id.is_empty() {
        return invalid("tenant_id is required");
    }
    if ir.signature.is_empty() || ir.steps.is_empty() {
        return invalid("signature and steps must be non-empty");
    }
    if ir.signature.iter().any(|part| part.is_empty()) {
        return invalid("signature actions must be named");
    }
    if ir.steps.len() != ir.signature.len() {
        return invalid("steps and signature differ in length");
    }
    for (index, step) in ir.steps.iter().enumerate() {
        if step.action_kind != ir.signature[index] {
            return invalid("step action does not match the signature");
        }
        if step.effect != "pure" && step.effect != "read_only" {
            return invalid("step effect must be pure or read_only");
        }
    }
    let mut expected_effects: Vec<&str> =
        ir.steps.iter().map(|step| step.effect.as_str()).collect();
    expected_effects.sort_unstable();
    expected_effects.dedup();
    let recorded: Vec<&str> = ir.side_effects.iter().map(String::as_str).collect();
    if recorded != expected_effects {
        return invalid("side_effects must list the sorted step effects");
    }
    if ir.table.is_empty() {
        return invalid("table must contain a row");
    }
    let mut previous: Option<&Vec<String>> = None;
    for row in &ir.table {
        if row.input.len() != ir.steps.len() || row.output.len() != ir.steps.len() {
            return invalid("table row width must match the steps");
        }
        for digest in row.input.iter().chain(row.output.iter()) {
            if !is_digest(digest) {
                return invalid("table digest must be sha256:<64 hex>");
            }
        }
        if let Some(seen) = previous {
            if &row.input <= seen {
                return invalid("table inputs must be sorted and unique");
            }
        }
        previous = Some(&row.input);
    }
    if !is_digest(&ir.environment.digest) {
        return invalid("environment digest must be sha256:<64 hex>");
    }
    if ir.environment.digest != ir.precondition.environment_eq {
        return invalid("environment digest must equal the precondition");
    }
    if !ir.precondition.input_in_table {
        return invalid("precondition requires the input to be in the table");
    }
    if !ir.postcondition.output_from_table || !ir.postcondition.equal_digest {
        return invalid("postcondition requires the table output");
    }
    if ir.schedule != SCHEDULE_INTERPRET {
        return invalid("schedule must be interpret");
    }
    if ir.fallback != FALLBACK_AGENT {
        return invalid("fallback must be original_agent_workflow");
    }
    let all_pure = ir.steps.iter().all(|step| step.effect == "pure");
    let expected_idempotency = if all_pure { "pure" } else { "read_only" };
    if ir.idempotency != expected_idempotency {
        return invalid("idempotency does not match the step effects");
    }
    let expected_attempts = if all_pure { 3 } else { 1 };
    if ir.retry.max_attempts != expected_attempts {
        return invalid("retry max_attempts does not match the step effects");
    }
    if ir.evidence.economics_minor <= 0 {
        return invalid("economics must be positive");
    }
    if ir.evidence.observation_ids.is_empty() {
        return invalid("observation ids are required");
    }
    if ir.evidence.agreement_considered == 0 {
        return invalid("agreement has no executions");
    }
    match ir.pressure.as_str() {
        "high" => {
            if ir.evidence.agent_executions < 30 {
                return invalid("high pressure requires 30 agent executions");
            }
            if !agreement_holds(
                ir.evidence.agreement_matching,
                ir.evidence.agreement_considered,
                99,
            ) {
                return invalid("high pressure requires 99 percent agreement");
            }
        }
        "low" => {
            if ir.evidence.agent_executions < 15 {
                return invalid("low pressure requires 15 agent executions");
            }
            if !agreement_holds(
                ir.evidence.agreement_matching,
                ir.evidence.agreement_considered,
                90,
            ) {
                return invalid("low pressure requires 90 percent agreement");
            }
        }
        _ => return invalid("pressure must be high or low"),
    }
    let expected_id = format!("{}:{}", ir.tenant_id, ir.signature.join(","));
    if ir.current_id != expected_id {
        return invalid("current_id must be tenant and signature");
    }
    Ok(())
}

fn agreement_holds(matching: u64, considered: u64, percent: u64) -> bool {
    if considered == 0 {
        return false;
    }
    u128::from(matching) * 100 >= u128::from(considered) * u128::from(percent)
}

pub(crate) fn is_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.chars().all(|character| character.is_ascii_hexdigit())
}

fn invalid(message: impl Into<String>) -> Result<(), IrError> {
    Err(IrError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: &str) -> String {
        format!("sha256:{byte}")
    }

    fn accepted() -> CurrentIr {
        let input = digest(&"11".repeat(32));
        let output = digest(&"22".repeat(32));
        let environment = digest(&"33".repeat(32));
        CurrentIr {
            schema: CIR_V0.to_string(),
            current_id: "tenant-a:read".to_string(),
            version: 1,
            tenant_id: "tenant-a".to_string(),
            signature: vec!["read".to_string()],
            pressure: "high".to_string(),
            steps: vec![Step {
                action_kind: "read".to_string(),
                effect: "read_only".to_string(),
            }],
            table: vec![IoRow {
                input: vec![input],
                output: vec![output],
            }],
            precondition: Precondition {
                environment_eq: environment.clone(),
                input_in_table: true,
            },
            postcondition: Postcondition {
                output_from_table: true,
                equal_digest: true,
            },
            environment: Environment {
                digest: environment,
            },
            side_effects: vec!["read_only".to_string()],
            retry: Retry { max_attempts: 1 },
            idempotency: "read_only".to_string(),
            schedule: SCHEDULE_INTERPRET.to_string(),
            fallback: FALLBACK_AGENT.to_string(),
            evidence: Evidence {
                agent_executions: 30,
                agreement_matching: 30,
                agreement_considered: 30,
                economics_minor: 1,
                observation_ids: (0..30).map(|index| format!("agent-{index}")).collect(),
            },
        }
    }

    #[test]
    fn a_read_only_current_validates() {
        assert_eq!(validate(&accepted()), Ok(()));
    }

    #[test]
    fn a_native_schedule_is_rejected() {
        let mut ir = accepted();
        ir.schedule = "native".to_string();
        let error = validate(&ir).unwrap_err();
        assert!(error.to_string().contains("interpret"), "{error}");
    }

    #[test]
    fn a_filesystem_effect_is_rejected() {
        let mut ir = accepted();
        ir.steps[0].effect = "filesystem_mutation".to_string();
        let error = validate(&ir).unwrap_err();
        assert!(error.to_string().contains("pure"), "{error}");
    }

    #[test]
    fn an_empty_table_is_rejected() {
        let mut ir = accepted();
        ir.table.clear();
        let error = validate(&ir).unwrap_err();
        assert!(error.to_string().contains("table"), "{error}");
    }

    #[test]
    fn zero_economics_is_rejected() {
        let mut ir = accepted();
        ir.evidence.economics_minor = 0;
        let error = validate(&ir).unwrap_err();
        assert!(error.to_string().contains("economics"), "{error}");
    }

    #[test]
    fn a_short_signature_is_rejected() {
        let mut ir = accepted();
        ir.signature.push("read".to_string());
        ir.current_id = "tenant-a:read,read".to_string();
        let error = validate(&ir).unwrap_err();
        assert!(error.to_string().contains("length"), "{error}");
    }

    #[test]
    fn high_pressure_with_fifteen_executions_is_rejected() {
        let mut ir = accepted();
        ir.evidence.agent_executions = 15;
        let error = validate(&ir).unwrap_err();
        assert!(error.to_string().contains("30"), "{error}");
    }
}
