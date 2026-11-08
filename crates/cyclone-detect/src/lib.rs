//! Segment non-generative successes and mine them into signature candidates.
//!
//! A segment stops at a model call, a control event, or an unsuccessful
//! action. Agreement counts agent executions that produced a segment.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use cyclone_observe::{ActionObservation, ActionOutcome, EffectClass, ObservationSource, Trace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub execution_id: String,
    pub tenant_id: String,
    pub source: ObservationSource,
    pub start_sequence: u64,
    pub end_sequence: u64,
    pub observation_ids: Vec<String>,
    pub signature: Vec<String>,
    pub effect_classes: Vec<EffectClass>,
    pub input_digests: Vec<Option<String>>,
    pub output_digests: Vec<Option<String>>,
    pub environment_digests: Vec<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterSpace {
    Fixed,
    Bounded,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Determinism {
    Held,
    Conflict,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentStability {
    Stable,
    Conflict,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub signature: Vec<String>,
    pub segments: Vec<Segment>,
    pub agent_executions: u64,
    pub mechanical_executions: u64,
    pub agreement_matching: u64,
    pub agreement_considered: u64,
    pub parameter_space: ParameterSpace,
    pub deterministic: Determinism,
    pub unknown_side_effects: bool,
    pub environment: EnvironmentStability,
}

pub fn segment_traces(traces: &[Trace]) -> Vec<Segment> {
    let mut segments = Vec::new();
    for trace in traces {
        let mut open: Vec<&ActionObservation> = Vec::new();
        for observation in &trace.observations {
            if is_boundary(observation) {
                push_segment(&mut segments, &open);
                open.clear();
                continue;
            }
            open.push(observation);
        }
        push_segment(&mut segments, &open);
    }
    segments
}

pub fn mine(segments: &[Segment]) -> Vec<Candidate> {
    let mut considered: BTreeSet<&str> = BTreeSet::new();
    for segment in segments {
        if segment.source == ObservationSource::Agent {
            considered.insert(segment.execution_id.as_str());
        }
    }
    let agreement_considered = considered.len() as u64;

    let mut grouped: BTreeMap<&[String], Vec<&Segment>> = BTreeMap::new();
    for segment in segments {
        grouped
            .entry(segment.signature.as_slice())
            .or_default()
            .push(segment);
    }

    let mut candidates = Vec::with_capacity(grouped.len());
    for (signature, group) in grouped {
        let mut agent_ids: BTreeSet<&str> = BTreeSet::new();
        let mut mechanical_ids: BTreeSet<&str> = BTreeSet::new();
        for segment in &group {
            match segment.source {
                ObservationSource::Agent => {
                    agent_ids.insert(segment.execution_id.as_str());
                }
                ObservationSource::Mechanical => {
                    mechanical_ids.insert(segment.execution_id.as_str());
                }
            }
        }
        let owned: Vec<Segment> = group.into_iter().cloned().collect();
        candidates.push(Candidate {
            signature: signature.to_vec(),
            agent_executions: agent_ids.len() as u64,
            mechanical_executions: mechanical_ids.len() as u64,
            agreement_matching: agent_ids.len() as u64,
            agreement_considered,
            parameter_space: parameter_space(&owned),
            deterministic: determinism(&owned),
            unknown_side_effects: owned.iter().any(|segment| {
                segment
                    .effect_classes
                    .iter()
                    .any(|effect| *effect == EffectClass::Unknown)
            }),
            environment: environment(&owned),
            segments: owned,
        });
    }
    candidates
}

fn is_boundary(observation: &ActionObservation) -> bool {
    observation.generative
        || observation.effect_class == EffectClass::Generative
        || observation.effect_class == EffectClass::Control
        || observation.outcome != ActionOutcome::Success
}

fn push_segment(out: &mut Vec<Segment>, open: &[&ActionObservation]) {
    if open.is_empty() {
        return;
    }
    let first = open[0];
    let last = open[open.len() - 1];
    out.push(Segment {
        execution_id: first.execution_id.clone(),
        tenant_id: first.tenant_id.clone(),
        source: first.source,
        start_sequence: first.sequence,
        end_sequence: last.sequence,
        observation_ids: open.iter().map(|row| row.observation_id.clone()).collect(),
        signature: open.iter().map(|row| row.action_kind.clone()).collect(),
        effect_classes: open.iter().map(|row| row.effect_class).collect(),
        input_digests: open.iter().map(|row| row.input_digest.clone()).collect(),
        output_digests: open.iter().map(|row| row.output_digest.clone()).collect(),
        environment_digests: open
            .iter()
            .map(|row| row.environment_digest.clone())
            .collect(),
    });
}

fn parameter_space(segments: &[Segment]) -> ParameterSpace {
    let mut tuples: BTreeSet<Vec<String>> = BTreeSet::new();
    for segment in segments {
        let mut tuple = Vec::with_capacity(segment.input_digests.len());
        for digest in &segment.input_digests {
            let Some(digest) = digest else {
                return ParameterSpace::Unknown;
            };
            tuple.push(digest.clone());
        }
        tuples.insert(tuple);
    }
    match tuples.len() {
        1 => ParameterSpace::Fixed,
        0 => ParameterSpace::Unknown,
        _ => ParameterSpace::Bounded,
    }
}

fn determinism(segments: &[Segment]) -> Determinism {
    let mut outputs: BTreeMap<Vec<String>, BTreeSet<Vec<String>>> = BTreeMap::new();
    for segment in segments {
        let Some(inputs) = present(&segment.input_digests) else {
            return Determinism::Unknown;
        };
        let Some(produced) = present(&segment.output_digests) else {
            return Determinism::Unknown;
        };
        outputs.entry(inputs).or_default().insert(produced);
    }
    if outputs.values().any(|set| set.len() > 1) {
        Determinism::Conflict
    } else {
        Determinism::Held
    }
}

fn environment(segments: &[Segment]) -> EnvironmentStability {
    let mut digests: BTreeSet<String> = BTreeSet::new();
    for segment in segments {
        for digest in &segment.environment_digests {
            let Some(digest) = digest else {
                return EnvironmentStability::Unknown;
            };
            digests.insert(digest.clone());
        }
    }
    match digests.len() {
        1 => EnvironmentStability::Stable,
        _ => EnvironmentStability::Conflict,
    }
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
    use cyclone_observe::ActionObservation;

    fn trace(observations: Vec<ActionObservation>) -> Trace {
        Trace {
            trace_id: observations[0].trace_id.clone(),
            tenant_id: observations[0].tenant_id.clone(),
            execution_id: observations[0].execution_id.clone(),
            source: observations[0].source,
            observations,
        }
    }

    #[test]
    fn a_model_call_splits_reads_inside_one_execution() {
        let mut middle = ActionObservation::new("exec", 2, "model.response");
        middle.observation_id = "m".to_string();
        let mut first = ActionObservation::new("exec", 1, "read");
        first.observation_id = "a".to_string();
        let mut last = ActionObservation::new("exec", 3, "read");
        last.observation_id = "b".to_string();
        let segments = segment_traces(&[trace(vec![first, middle, last])]);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].signature, vec!["read".to_string()]);
        assert_eq!(segments[1].signature, vec!["read".to_string()]);
        let mined = mine(&segments);
        assert_eq!(mined.len(), 1);
        assert_eq!(mined[0].agent_executions, 1);
        assert_eq!(mined[0].agreement_matching, 1);
        assert_eq!(mined[0].agreement_considered, 1);
    }

    #[test]
    fn agreement_uses_agent_executions_that_produced_a_segment() {
        let reads = (0..3).map(|index| ActionObservation::new(format!("read-{index}"), 1, "read"));
        let mut write = ActionObservation::new("write-1", 1, "write_file");
        write.tenant_id = "tenant".to_string();
        let traces = reads
            .chain(std::iter::once(write))
            .map(|observation| trace(vec![observation]))
            .collect::<Vec<_>>();
        let mined = mine(&segment_traces(&traces));
        let read = mined
            .iter()
            .find(|candidate| candidate.signature == vec!["read".to_string()])
            .unwrap();
        assert_eq!(read.agreement_matching, 3);
        assert_eq!(read.agreement_considered, 4);
        let write = mined
            .iter()
            .find(|candidate| candidate.signature == vec!["write_file".to_string()])
            .unwrap();
        assert_eq!(write.agreement_matching, 1);
        assert_eq!(write.agreement_considered, 4);
    }
}
