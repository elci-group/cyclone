//! Shadow equivalence for an interpret-only current.
//!
//! The interpreter looks up a recorded digest. It performs one lookup. The
//! retry count on the current is evidence, and this crate leaves it unread
//! during interpretation. A missing input or a different environment resolves
//! to the original agent workflow.

#![forbid(unsafe_code)]

use cyclone_detect::Candidate;
use cyclone_ir::{validate, CurrentIr, IrError};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error(transparent)]
    Invalid(#[from] IrError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpretResult {
    Value { output: Vec<String> },
    Fallback { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShadowReport {
    pub current_id: String,
    pub compared: u64,
    pub matched: u64,
    pub mismatched: u64,
    pub pass: bool,
}

pub fn interpret(
    ir: &CurrentIr,
    input: &[String],
    environment: &str,
) -> Result<InterpretResult, VerifyError> {
    validate(ir)?;
    if environment != ir.environment.digest {
        return Ok(InterpretResult::Fallback {
            reason: "environment mismatch".to_string(),
        });
    }
    if let Some(row) = ir.table.iter().find(|row| row.input == input) {
        return Ok(InterpretResult::Value {
            output: row.output.clone(),
        });
    }
    Ok(InterpretResult::Fallback {
        reason: "input not in table".to_string(),
    })
}

pub fn shadow(ir: &CurrentIr, candidate: &Candidate) -> Result<ShadowReport, VerifyError> {
    validate(ir)?;
    let mut compared = 0_u64;
    let mut matched = 0_u64;
    let mut mismatched = 0_u64;
    for segment in &candidate.segments {
        compared += 1;
        let Some(environment) = one_environment(&segment.environment_digests) else {
            mismatched += 1;
            continue;
        };
        let Some(input) = present(&segment.input_digests) else {
            mismatched += 1;
            continue;
        };
        let Some(expected) = present(&segment.output_digests) else {
            mismatched += 1;
            continue;
        };
        match interpret(ir, &input, &environment)? {
            InterpretResult::Value { output } if output == expected => matched += 1,
            _ => mismatched += 1,
        }
    }
    let pass = compared > 0 && matched == compared && mismatched == 0;
    Ok(ShadowReport {
        current_id: ir.current_id.clone(),
        compared,
        matched,
        mismatched,
        pass,
    })
}

fn one_environment(digests: &[Option<String>]) -> Option<String> {
    let mut found: Option<String> = None;
    for digest in digests {
        let digest = digest.clone()?;
        match &found {
            None => found = Some(digest),
            Some(current) if current == &digest => {}
            Some(_) => return None,
        }
    }
    found
}

fn present(digests: &[Option<String>]) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(digests.len());
    for digest in digests {
        out.push(digest.clone()?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyclone_detect::{Candidate, Determinism, EnvironmentStability, ParameterSpace, Segment};
    use cyclone_ir::{
        Environment, Evidence, IoRow, Postcondition, Precondition, Retry, Step, CIR_V0,
        FALLBACK_AGENT, SCHEDULE_INTERPRET,
    };
    use cyclone_observe::{EffectClass, ObservationSource};

    fn digest(byte: &str) -> String {
        format!("sha256:{byte}")
    }

    fn current() -> (CurrentIr, String, String, String) {
        let input = digest(&"11".repeat(32));
        let output = digest(&"22".repeat(32));
        let environment = digest(&"33".repeat(32));
        let ir = CurrentIr {
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
                input: vec![input.clone()],
                output: vec![output.clone()],
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
                digest: environment.clone(),
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
                observation_ids: vec!["agent-0".to_string()],
            },
        };
        (ir, input, output, environment)
    }

    fn candidate(input: &str, output: &str, environment: &str) -> Candidate {
        Candidate {
            signature: vec!["read".to_string()],
            segments: vec![Segment {
                execution_id: "exec-0".to_string(),
                tenant_id: "tenant-a".to_string(),
                source: ObservationSource::Agent,
                start_sequence: 1,
                end_sequence: 1,
                observation_ids: vec!["agent-0".to_string()],
                signature: vec!["read".to_string()],
                effect_classes: vec![EffectClass::ReadOnly],
                input_digests: vec![Some(input.to_string())],
                output_digests: vec![Some(output.to_string())],
                environment_digests: vec![Some(environment.to_string())],
            }],
            agent_executions: 30,
            mechanical_executions: 0,
            agreement_matching: 30,
            agreement_considered: 30,
            parameter_space: ParameterSpace::Fixed,
            deterministic: Determinism::Held,
            unknown_side_effects: false,
            environment: EnvironmentStability::Stable,
        }
    }

    #[test]
    fn a_novel_input_falls_back() {
        let (ir, _input, _output, environment) = current();
        let novel = digest(&"ab".repeat(32));
        let result = interpret(&ir, &[novel], &environment).unwrap();
        assert!(
            matches!(result, InterpretResult::Fallback { ref reason } if reason.contains("input")),
            "{result:?}"
        );
    }

    #[test]
    fn an_environment_mismatch_returns_no_output() {
        let (ir, input, recorded, _environment) = current();
        let other = digest(&"44".repeat(32));
        let result = interpret(&ir, &[input], &other).unwrap();
        match &result {
            InterpretResult::Fallback { reason } => {
                assert!(reason.contains("environment"), "{reason}");
            }
            InterpretResult::Value { output } => {
                assert_ne!(output, &vec![recorded]);
                assert!(
                    output.is_empty(),
                    "environment mismatch returned {output:?}"
                );
            }
        }
    }

    #[test]
    fn a_mutated_table_fails_shadow() {
        let (mut ir, input, output, environment) = current();
        ir.table[0].output = vec![digest(&"55".repeat(32))];
        let report = shadow(&ir, &candidate(&input, &output, &environment)).unwrap();
        assert!(!report.pass);
        assert_eq!(report.compared, 1);
        assert_eq!(report.matched, 0);
        assert_eq!(report.mismatched, 1);
    }
}
