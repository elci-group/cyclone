//! Classify mined candidates with the provisional CTD-001 gates.
//!
//! A gate that is unmeasured fails. Mechanical replicates do not increment
//! the observation count. Passing gates makes a candidate eligible. It does
//! not by itself deploy a current.

#![forbid(unsafe_code)]

use cyclone_detect::{Candidate, Determinism, EnvironmentStability, ParameterSpace};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const HIGH_OBSERVATIONS: u64 = 30;
pub const LOW_OBSERVATIONS: u64 = 15;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PressureError {
    #[error("evidence table is invalid JSON")]
    Json,
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pressure {
    High,
    Low,
    Gust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Promotion {
    Eligible,
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayEvidence {
    Unmeasured,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gate {
    pub name: String,
    pub high_pass: bool,
    pub low_pass: bool,
    pub observed: String,
    pub high_requirement: String,
    pub low_requirement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Classification {
    pub signature: Vec<String>,
    pub pressure: Pressure,
    pub promotion: Promotion,
    pub agent_executions: u64,
    pub mechanical_executions: u64,
    pub agreement_matching: u64,
    pub agreement_considered: u64,
    pub observation_ids: Vec<String>,
    pub gates: Vec<Gate>,
    pub explanation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceTable {
    entries: Vec<EvidenceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvidenceEntry {
    signature: Vec<String>,
    replay: ReplayEvidence,
    economics_minor: Option<i64>,
}

impl EvidenceTable {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, PressureError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            schema: String,
            entries: Vec<RawEntry>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawEntry {
            signature: Vec<String>,
            replay: ReplayEvidence,
            #[serde(default)]
            economics_minor: Option<i64>,
        }
        let file: File = serde_json::from_str(text).map_err(|_| PressureError::Json)?;
        if file.schema != "cyclone.evidence-table/v0" {
            return Err(PressureError::Message(
                "evidence schema must be cyclone.evidence-table/v0".to_string(),
            ));
        }
        let mut seen = Vec::new();
        let mut entries = Vec::new();
        for entry in file.entries {
            if entry.signature.is_empty() || entry.signature.iter().any(|part| part.is_empty()) {
                return Err(PressureError::Message(
                    "evidence signature must name at least one action".to_string(),
                ));
            }
            if seen.contains(&entry.signature) {
                return Err(PressureError::Message(format!(
                    "evidence repeats signature {}",
                    entry.signature.join(",")
                )));
            }
            seen.push(entry.signature.clone());
            entries.push(EvidenceEntry {
                signature: entry.signature,
                replay: entry.replay,
                economics_minor: entry.economics_minor,
            });
        }
        Ok(Self { entries })
    }

    pub fn lookup(&self, signature: &[String]) -> (ReplayEvidence, Option<i64>) {
        self.entries
            .iter()
            .find(|entry| entry.signature == signature)
            .map(|entry| (entry.replay, entry.economics_minor))
            .unwrap_or((ReplayEvidence::Unmeasured, None))
    }
}

pub fn classify(
    candidate: &Candidate,
    replay: ReplayEvidence,
    economics_minor: Option<i64>,
) -> Classification {
    let gates = vec![
        observations_gate(candidate),
        agreement_gate(candidate),
        replay_gate(replay),
        parameter_gate(candidate.parameter_space),
        determinism_gate(candidate.deterministic),
        economics_gate(economics_minor),
        side_effect_gate(candidate.unknown_side_effects),
        environment_gate(candidate.environment),
    ];
    let high = gates.iter().all(|gate| gate.high_pass);
    let low = gates.iter().all(|gate| gate.low_pass);
    let (pressure, promotion) = if high {
        (Pressure::High, Promotion::Eligible)
    } else if low {
        (Pressure::Low, Promotion::Eligible)
    } else {
        (Pressure::Gust, Promotion::Refused)
    };
    let mut explanation = Vec::new();
    match pressure {
        Pressure::High => explanation.push("provisional high-pressure gates passed".to_string()),
        Pressure::Low => {
            explanation.push("provisional low-pressure gates passed".to_string());
            for gate in &gates {
                if !gate.high_pass {
                    explanation.push(format!(
                        "{}: observed {}, high pressure requires {}",
                        gate.name, gate.observed, gate.high_requirement
                    ));
                }
            }
        }
        Pressure::Gust => {
            explanation.push("promotion refused".to_string());
            for gate in &gates {
                if !gate.low_pass {
                    explanation.push(format!(
                        "{}: observed {}, low pressure requires {}",
                        gate.name, gate.observed, gate.low_requirement
                    ));
                }
            }
        }
    }
    let mut observation_ids = Vec::new();
    for segment in &candidate.segments {
        observation_ids.extend(segment.observation_ids.iter().cloned());
    }
    Classification {
        signature: candidate.signature.clone(),
        pressure,
        promotion,
        agent_executions: candidate.agent_executions,
        mechanical_executions: candidate.mechanical_executions,
        agreement_matching: candidate.agreement_matching,
        agreement_considered: candidate.agreement_considered,
        observation_ids,
        gates,
        explanation,
    }
}

fn observations_gate(candidate: &Candidate) -> Gate {
    Gate {
        name: "observations".to_string(),
        high_pass: candidate.agent_executions >= HIGH_OBSERVATIONS,
        low_pass: candidate.agent_executions >= LOW_OBSERVATIONS,
        observed: format!(
            "{} agent, {} mechanical",
            candidate.agent_executions, candidate.mechanical_executions
        ),
        high_requirement: format!(">={HIGH_OBSERVATIONS} agent executions"),
        low_requirement: format!(">={LOW_OBSERVATIONS} agent executions"),
    }
}

fn agreement_gate(candidate: &Candidate) -> Gate {
    let considered = candidate.agreement_considered;
    let matching = candidate.agreement_matching;
    let (high_pass, low_pass) = if considered == 0 {
        (false, false)
    } else {
        (
            matching.saturating_mul(100) >= considered.saturating_mul(99),
            matching.saturating_mul(100) >= considered.saturating_mul(90),
        )
    };
    Gate {
        name: "structural_agreement".to_string(),
        high_pass,
        low_pass,
        observed: format!("{matching}/{considered}"),
        high_requirement: ">=99/100 of agent executions that produced a segment".to_string(),
        low_requirement: ">=90/100 of agent executions that produced a segment".to_string(),
    }
}

fn replay_gate(replay: ReplayEvidence) -> Gate {
    let passed = replay == ReplayEvidence::Passed;
    Gate {
        name: "replay".to_string(),
        high_pass: passed,
        low_pass: passed,
        observed: match replay {
            ReplayEvidence::Passed => "passed",
            ReplayEvidence::Failed => "failed",
            ReplayEvidence::Unmeasured => "unmeasured",
        }
        .to_string(),
        high_requirement: "passed".to_string(),
        low_requirement: "passed".to_string(),
    }
}

fn parameter_gate(space: ParameterSpace) -> Gate {
    Gate {
        name: "parameters".to_string(),
        high_pass: space == ParameterSpace::Fixed,
        low_pass: matches!(space, ParameterSpace::Fixed | ParameterSpace::Bounded),
        observed: match space {
            ParameterSpace::Fixed => "fixed",
            ParameterSpace::Bounded => "bounded",
            ParameterSpace::Unknown => "unknown",
        }
        .to_string(),
        high_requirement: "fixed".to_string(),
        low_requirement: "fixed or bounded".to_string(),
    }
}

fn determinism_gate(value: Determinism) -> Gate {
    let held = value == Determinism::Held;
    Gate {
        name: "determinism".to_string(),
        high_pass: held,
        low_pass: held,
        observed: match value {
            Determinism::Held => "held",
            Determinism::Conflict => "conflict",
            Determinism::Unknown => "unknown",
        }
        .to_string(),
        high_requirement: "held".to_string(),
        low_requirement: "held".to_string(),
    }
}

fn economics_gate(minor: Option<i64>) -> Gate {
    let pass = minor.is_some_and(|value| value > 0);
    Gate {
        name: "economics".to_string(),
        high_pass: pass,
        low_pass: pass,
        observed: minor
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unmeasured".to_string()),
        high_requirement: ">0".to_string(),
        low_requirement: ">0".to_string(),
    }
}

fn side_effect_gate(unknown: bool) -> Gate {
    Gate {
        name: "side_effects".to_string(),
        high_pass: !unknown,
        low_pass: !unknown,
        observed: if unknown {
            "unknown".to_string()
        } else {
            "declared".to_string()
        },
        high_requirement: "none unknown".to_string(),
        low_requirement: "none unknown".to_string(),
    }
}

fn environment_gate(value: EnvironmentStability) -> Gate {
    let stable = value == EnvironmentStability::Stable;
    Gate {
        name: "environment".to_string(),
        high_pass: stable,
        low_pass: stable,
        observed: match value {
            EnvironmentStability::Stable => "stable",
            EnvironmentStability::Conflict => "conflict",
            EnvironmentStability::Unknown => "unknown",
        }
        .to_string(),
        high_requirement: "one digest".to_string(),
        low_requirement: "one digest".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyclone_detect::{mine, segment_traces};
    use cyclone_observe::{ActionObservation, Trace};

    fn digest(byte: u8) -> String {
        format!("sha256:{}", format!("{byte:02x}").repeat(32))
    }

    fn read_trace(name: &str, input: u8, environment: u8) -> Trace {
        let observation = ActionObservation::new(name, 1, "read").with_digests(
            &digest(input),
            &digest(0x22),
            &digest(environment),
        );
        Trace {
            trace_id: observation.trace_id.clone(),
            tenant_id: observation.tenant_id.clone(),
            execution_id: observation.execution_id.clone(),
            source: observation.source,
            observations: vec![observation],
        }
    }

    fn classify_reads(
        count: usize,
        input: impl Fn(usize) -> u8,
        environment: u8,
    ) -> Classification {
        let traces: Vec<Trace> = (0..count)
            .map(|index| read_trace(&format!("exec-{index}"), input(index), environment))
            .collect();
        let candidates = mine(&segment_traces(&traces));
        assert_eq!(candidates.len(), 1);
        classify(&candidates[0], ReplayEvidence::Passed, Some(1))
    }

    #[test]
    fn thirty_fixed_reads_are_high_when_replay_and_economics_are_attached() {
        let classification = classify_reads(30, |_| 0x11, 0x33);
        assert_eq!(classification.pressure, Pressure::High);
        assert_eq!(classification.promotion, Promotion::Eligible);
    }

    #[test]
    fn fifteen_bounded_reads_are_low() {
        let classification =
            classify_reads(15, |index| if index % 2 == 0 { 0x11 } else { 0x12 }, 0x33);
        assert_eq!(classification.pressure, Pressure::Low);
        assert_eq!(classification.promotion, Promotion::Eligible);
        assert!(classification
            .explanation
            .iter()
            .any(|line| line.contains("parameters")));
    }

    #[test]
    fn unmeasured_economics_refuses_a_large_sample() {
        let traces: Vec<Trace> = (0..30)
            .map(|index| read_trace(&format!("e{index}"), 0x11, 0x33))
            .collect();
        let candidate = &mine(&segment_traces(&traces))[0];
        let classification = classify(candidate, ReplayEvidence::Passed, None);
        assert_eq!(classification.pressure, Pressure::Gust);
        assert_eq!(classification.promotion, Promotion::Refused);
    }

    #[test]
    fn evidence_table_rejects_unknown_top_level_fields() {
        let error = EvidenceTable::parse(
            r#"{"schema":"cyclone.evidence-table/v0","entries":[],"comment":"ignored before"}"#,
        )
        .unwrap_err();
        assert_eq!(error, PressureError::Json);
    }

    #[test]
    fn evidence_table_rejects_unknown_entry_fields() {
        let error = EvidenceTable::parse(
            r#"{"schema":"cyclone.evidence-table/v0","entries":[{"signature":["read"],"replay":"passed","economics_minor":1,"note":"ignored before"}]}"#,
        )
        .unwrap_err();
        assert_eq!(error, PressureError::Json);
    }

    #[test]
    fn unknown_side_effects_refuse_promotion() {
        let traces: Vec<Trace> = (0..30)
            .map(|index| {
                let observation = ActionObservation::new(format!("exec-{index}"), 1, "run_checks")
                    .with_digests(&digest(1), &digest(2), &digest(3));
                Trace {
                    trace_id: observation.trace_id.clone(),
                    tenant_id: observation.tenant_id.clone(),
                    execution_id: observation.execution_id.clone(),
                    source: observation.source,
                    observations: vec![observation],
                }
            })
            .collect();
        let candidate = &mine(&segment_traces(&traces))[0];
        let classification = classify(candidate, ReplayEvidence::Passed, Some(1));
        assert_eq!(classification.promotion, Promotion::Refused);
        assert!(classification
            .gates
            .iter()
            .any(|gate| gate.name == "side_effects" && !gate.low_pass));
    }
}
