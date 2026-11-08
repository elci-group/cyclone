//! Compile an eligible pure or read-only candidate into `cyclone.cir/v0`.
//!
//! Eligibility from the provisional gates is one input. This crate also
//! requires every step effect to be pure or read-only, then validates the
//! intermediate form. A refusal is a value. The lookup table records retry
//! policy and leaves execution of that policy to a later runtime.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use cyclone_detect::{Candidate, Determinism, EnvironmentStability, ParameterSpace};
use cyclone_ir::{
    validate, CurrentIr, Environment, Evidence, IoRow, Postcondition, Precondition, Retry, Step,
    CIR_V0, FALLBACK_AGENT, SCHEDULE_INTERPRET,
};
use cyclone_observe::EffectClass;
use cyclone_pressure::{Classification, Pressure, Promotion};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub signature: Vec<String>,
    pub reason: String,
}

pub fn compile(
    candidate: &Candidate,
    classification: &Classification,
    economics_minor: i64,
) -> Result<CurrentIr, Refusal> {
    if candidate.signature != classification.signature {
        return Err(refuse(candidate, "signature mismatch"));
    }
    if candidate.segments.is_empty() {
        return Err(refuse(candidate, "empty segments"));
    }
    if classification.promotion != Promotion::Eligible {
        return Err(refuse(candidate, "promotion refused"));
    }
    if classification.pressure == Pressure::Gust {
        return Err(refuse(candidate, "gust pressure"));
    }
    for segment in &candidate.segments {
        if segment
            .effect_classes
            .iter()
            .any(|effect| !matches!(effect, EffectClass::Pure | EffectClass::ReadOnly))
        {
            return Err(refuse(candidate, "effect class is not pure or read_only"));
        }
    }
    if candidate.unknown_side_effects {
        return Err(refuse(candidate, "unknown side effects"));
    }
    if candidate.deterministic != Determinism::Held {
        return Err(refuse(candidate, "determinism refused"));
    }
    if candidate.environment != EnvironmentStability::Stable {
        return Err(refuse(candidate, "environment refused"));
    }
    if candidate.parameter_space == ParameterSpace::Unknown {
        return Err(refuse(candidate, "parameter space unknown"));
    }
    if classification.pressure == Pressure::High
        && candidate.parameter_space != ParameterSpace::Fixed
    {
        return Err(refuse(candidate, "high pressure requires fixed parameters"));
    }
    let Some(economics_gate) = classification
        .gates
        .iter()
        .find(|gate| gate.name == "economics")
    else {
        return Err(refuse(candidate, "economics gate missing"));
    };
    if economics_minor <= 0 || !(economics_gate.high_pass || economics_gate.low_pass) {
        return Err(refuse(candidate, "economics refused"));
    }
    let Some(replay_gate) = classification
        .gates
        .iter()
        .find(|gate| gate.name == "replay")
    else {
        return Err(refuse(candidate, "replay gate missing"));
    };
    if !(replay_gate.high_pass || replay_gate.low_pass) {
        return Err(refuse(candidate, "replay refused"));
    }

    let tenant = &candidate.segments[0].tenant_id;
    if candidate
        .segments
        .iter()
        .any(|segment| segment.tenant_id != *tenant)
    {
        return Err(refuse(candidate, "cross-tenant"));
    }
    let width = candidate.signature.len();
    for segment in &candidate.segments {
        if segment.signature != candidate.signature
            || segment.effect_classes.len() != width
            || segment.input_digests.len() != width
            || segment.output_digests.len() != width
            || segment.environment_digests.len() != width
        {
            return Err(refuse(candidate, "signature mismatch"));
        }
        if segment.input_digests.iter().any(Option::is_none)
            || segment.output_digests.iter().any(Option::is_none)
            || segment.environment_digests.iter().any(Option::is_none)
        {
            return Err(refuse(candidate, "digest missing"));
        }
    }

    let mut environment: Option<String> = None;
    for segment in &candidate.segments {
        for digest in &segment.environment_digests {
            let Some(digest) = digest.clone() else {
                return Err(refuse(candidate, "digest missing"));
            };
            match &environment {
                None => environment = Some(digest),
                Some(current) if current == &digest => {}
                Some(_) => return Err(refuse(candidate, "environment digest conflict")),
            }
        }
    }
    let Some(environment) = environment else {
        return Err(refuse(candidate, "digest missing"));
    };

    let canonical = &candidate.segments[0].effect_classes;
    for segment in &candidate.segments {
        if segment.effect_classes != *canonical {
            return Err(refuse(candidate, "effect class conflict"));
        }
    }
    let mut effects = Vec::with_capacity(width);
    for effect in canonical {
        let Some(label) = effect_label(*effect) else {
            return Err(refuse(candidate, "effect class is not pure or read_only"));
        };
        effects.push(label);
    }

    let mut table: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for segment in &candidate.segments {
        let Some(input) = present(&segment.input_digests) else {
            return Err(refuse(candidate, "digest missing"));
        };
        let Some(output) = present(&segment.output_digests) else {
            return Err(refuse(candidate, "digest missing"));
        };
        if let Some(existing) = table.get(&input) {
            if existing != &output {
                return Err(refuse(candidate, "output conflict"));
            }
        } else {
            table.insert(input, output);
        }
    }
    if classification.pressure == Pressure::High && table.len() != 1 {
        return Err(refuse(candidate, "high pressure requires one table row"));
    }

    let all_pure = effects.iter().all(|effect| *effect == "pure");
    let mut side_effects: Vec<String> =
        effects.iter().map(|effect| (*effect).to_string()).collect();
    side_effects.sort();
    side_effects.dedup();
    let steps = candidate
        .signature
        .iter()
        .zip(effects.iter())
        .map(|(action, effect)| Step {
            action_kind: action.clone(),
            effect: (*effect).to_string(),
        })
        .collect();
    let rows = table
        .into_iter()
        .map(|(input, output)| IoRow { input, output })
        .collect();
    let ir = CurrentIr {
        schema: CIR_V0.to_string(),
        current_id: format!("{tenant}:{}", candidate.signature.join(",")),
        version: 1,
        tenant_id: tenant.clone(),
        signature: candidate.signature.clone(),
        pressure: match classification.pressure {
            Pressure::High => "high",
            Pressure::Low => "low",
            Pressure::Gust => "gust",
        }
        .to_string(),
        steps,
        table: rows,
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
        side_effects,
        retry: Retry {
            max_attempts: if all_pure { 3 } else { 1 },
        },
        idempotency: if all_pure { "pure" } else { "read_only" }.to_string(),
        schedule: SCHEDULE_INTERPRET.to_string(),
        fallback: FALLBACK_AGENT.to_string(),
        evidence: Evidence {
            agent_executions: classification.agent_executions,
            agreement_matching: classification.agreement_matching,
            agreement_considered: classification.agreement_considered,
            economics_minor,
            observation_ids: classification.observation_ids.clone(),
        },
    };
    validate(&ir).map_err(|error| refuse(candidate, format!("validation: {error}")))?;
    Ok(ir)
}

fn effect_label(effect: EffectClass) -> Option<&'static str> {
    match effect {
        EffectClass::Pure => Some("pure"),
        EffectClass::ReadOnly => Some("read_only"),
        _ => None,
    }
}

fn present(digests: &[Option<String>]) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(digests.len());
    for digest in digests {
        out.push(digest.clone()?);
    }
    Some(out)
}

fn refuse(candidate: &Candidate, reason: impl Into<String>) -> Refusal {
    Refusal {
        signature: candidate.signature.clone(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyclone_detect::{mine, segment_traces, Determinism};
    use cyclone_ir::validate;
    use cyclone_observe::{ActionObservation, Trace};
    use cyclone_pressure::{classify, EvidenceTable, Pressure, Promotion, ReplayEvidence};
    use cyclone_verify::{interpret, shadow, InterpretResult};

    const INPUT: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const OUTPUT: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    const ENVIRONMENT: &str =
        "sha256:3333333333333333333333333333333333333333333333333333333333333333";
    const INPUT_B: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OUTPUT_B: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn trace_of(observations: Vec<ActionObservation>) -> Trace {
        Trace {
            trace_id: observations[0].trace_id.clone(),
            tenant_id: observations[0].tenant_id.clone(),
            execution_id: observations[0].execution_id.clone(),
            source: observations[0].source,
            observations,
        }
    }

    fn only_candidate(trace: &str) -> Candidate {
        let ingested = cyclone_observe::ingest_jsonl(trace).unwrap();
        let mined = mine(&segment_traces(&ingested.traces));
        assert_eq!(mined.len(), 1);
        mined.into_iter().next().unwrap()
    }

    fn repeated(kind: &str, count: usize, input: &str, output: &str) -> Candidate {
        let traces = (0..count)
            .map(|index| {
                let observation = ActionObservation::new(format!("exec-{index}"), 1, kind)
                    .with_digests(input, output, ENVIRONMENT);
                trace_of(vec![observation])
            })
            .collect::<Vec<_>>();
        let mined = mine(&segment_traces(&traces));
        assert_eq!(mined.len(), 1);
        mined.into_iter().next().unwrap()
    }

    #[test]
    fn reads_with_evidence_compile_and_shadow() {
        let candidate = only_candidate(include_str!("../../../traces/reads-30.jsonl"));
        let evidence =
            EvidenceTable::parse(include_str!("../../../traces/reads-30.evidence.json")).unwrap();
        let (replay, economics) = evidence.lookup(&candidate.signature);
        let classification = classify(&candidate, replay, economics);
        let ir = compile(&candidate, &classification, economics.unwrap()).unwrap();
        assert_eq!(validate(&ir), Ok(()));
        assert_eq!(ir.current_id, "tenant-a:read");
        assert_eq!(ir.table.len(), 1);
        assert_eq!(ir.evidence.observation_ids, classification.observation_ids);
        let report = shadow(&ir, &candidate).unwrap();
        assert!(report.pass);
        assert_eq!(
            (report.compared, report.matched, report.mismatched),
            (30, 30, 0)
        );
        let input = ir.table[0].input.clone();
        let environment = ir.environment.digest.clone();
        let first = interpret(&ir, &input, &environment).unwrap();
        let second = interpret(&ir, &input, &environment).unwrap();
        assert_eq!(first, second);
        assert!(matches!(first, InterpretResult::Value { .. }));
    }

    #[test]
    fn pure_with_evidence_compiles_and_shadows() {
        let candidate = only_candidate(include_str!("../../../traces/pure-30.jsonl"));
        let evidence =
            EvidenceTable::parse(include_str!("../../../traces/pure-30.evidence.json")).unwrap();
        let (replay, economics) = evidence.lookup(&candidate.signature);
        let classification = classify(&candidate, replay, economics);
        let ir = compile(&candidate, &classification, economics.unwrap()).unwrap();
        assert_eq!(ir.idempotency, "pure");
        assert_eq!(ir.retry.max_attempts, 3);
        let report = shadow(&ir, &candidate).unwrap();
        assert!(report.pass);
        assert_eq!(report.matched, 30);
    }

    #[test]
    fn missing_evidence_refuses_promotion() {
        let candidate = only_candidate(include_str!("../../../traces/reads-30.jsonl"));
        let classification = classify(&candidate, ReplayEvidence::Unmeasured, None);
        let refusal = compile(&candidate, &classification, 0).unwrap_err();
        assert_eq!(refusal.reason, "promotion refused");
    }

    #[test]
    fn a_split_environment_is_refused() {
        let candidate = only_candidate(include_str!("../../../traces/environment-split.jsonl"));
        let evidence =
            EvidenceTable::parse(include_str!("../../../traces/reads-30.evidence.json")).unwrap();
        let (replay, economics) = evidence.lookup(&candidate.signature);
        let classification = classify(&candidate, replay, economics);
        let refusal = compile(&candidate, &classification, economics.unwrap_or(0)).unwrap_err();
        assert_eq!(refusal.reason, "promotion refused");
    }

    #[test]
    fn the_lucid_fixture_has_nothing_to_compile() {
        let ingested =
            cyclone_observe::ingest_jsonl(include_str!("../../../traces/lucid-session-v1.jsonl"))
                .unwrap();
        let mined = mine(&segment_traces(&ingested.traces));
        assert!(mined.is_empty());
    }

    #[test]
    fn an_eligible_write_is_refused_at_the_effect_guard() {
        let candidate = repeated("write_file", 30, INPUT, OUTPUT);
        let mut classification = classify(&candidate, ReplayEvidence::Passed, Some(1));
        assert_eq!(classification.promotion, Promotion::Eligible);
        assert_eq!(classification.pressure, Pressure::High);
        classification.promotion = Promotion::Eligible;
        classification.pressure = Pressure::High;
        let refusal = compile(&candidate, &classification, 1).unwrap_err();
        assert_eq!(refusal.reason, "effect class is not pure or read_only");
    }

    #[test]
    fn a_two_step_pure_signature_keeps_both_digests() {
        let traces = (0..30)
            .map(|index| {
                let execution = format!("exec-{index}");
                let first = ActionObservation::new(&execution, 1, "pure").with_digests(
                    INPUT,
                    OUTPUT,
                    ENVIRONMENT,
                );
                let second = ActionObservation::new(&execution, 2, "pure").with_digests(
                    INPUT_B,
                    OUTPUT_B,
                    ENVIRONMENT,
                );
                trace_of(vec![first, second])
            })
            .collect::<Vec<_>>();
        let candidate = mine(&segment_traces(&traces)).into_iter().next().unwrap();
        assert_eq!(
            candidate.signature,
            vec!["pure".to_string(), "pure".to_string()]
        );
        let classification = classify(&candidate, ReplayEvidence::Passed, Some(1));
        let ir = compile(&candidate, &classification, 1).unwrap();
        assert_eq!(ir.steps.len(), 2);
        assert_eq!(ir.table.len(), 1);
        assert_eq!(ir.table[0].input.len(), 2);
        assert_eq!(ir.table[0].output.len(), 2);
        assert_eq!(ir.idempotency, "pure");
        assert_eq!(ir.retry.max_attempts, 3);
    }

    #[test]
    fn compiling_twice_yields_the_same_json() {
        let candidate = only_candidate(include_str!("../../../traces/reads-30.jsonl"));
        let evidence =
            EvidenceTable::parse(include_str!("../../../traces/reads-30.evidence.json")).unwrap();
        let (replay, economics) = evidence.lookup(&candidate.signature);
        let classification = classify(&candidate, replay, economics);
        let left = compile(&candidate, &classification, economics.unwrap()).unwrap();
        let right = compile(&candidate, &classification, economics.unwrap()).unwrap();
        let left_json = serde_json::to_string(&left).unwrap();
        let right_json = serde_json::to_string(&right).unwrap();
        assert_eq!(left_json, right_json);
    }

    #[test]
    fn low_pressure_keeps_a_bounded_table() {
        let traces = (0..15)
            .map(|index| {
                let (input, output) = if index % 2 == 0 {
                    (INPUT, OUTPUT)
                } else {
                    (INPUT_B, OUTPUT_B)
                };
                let observation = ActionObservation::new(format!("exec-{index}"), 1, "read")
                    .with_digests(input, output, ENVIRONMENT);
                trace_of(vec![observation])
            })
            .collect::<Vec<_>>();
        let candidate = mine(&segment_traces(&traces)).into_iter().next().unwrap();
        let classification = classify(&candidate, ReplayEvidence::Passed, Some(1));
        assert_eq!(classification.pressure, Pressure::Low);
        let ir = compile(&candidate, &classification, 1).unwrap();
        assert_eq!(ir.pressure, "low");
        assert_eq!(ir.table.len(), 2);
        assert_eq!(validate(&ir), Ok(()));
    }

    #[test]
    fn high_pressure_rejects_a_bounded_table() {
        let traces = (0..15)
            .map(|index| {
                let (input, output) = if index % 2 == 0 {
                    (INPUT, OUTPUT)
                } else {
                    (INPUT_B, OUTPUT_B)
                };
                let observation = ActionObservation::new(format!("exec-{index}"), 1, "read")
                    .with_digests(input, output, ENVIRONMENT);
                trace_of(vec![observation])
            })
            .collect::<Vec<_>>();
        let candidate = mine(&segment_traces(&traces)).into_iter().next().unwrap();
        let mut classification = classify(&candidate, ReplayEvidence::Passed, Some(1));
        classification.pressure = Pressure::High;
        classification.promotion = Promotion::Eligible;
        let refusal = compile(&candidate, &classification, 1).unwrap_err();
        assert_eq!(refusal.reason, "high pressure requires fixed parameters");
    }

    #[test]
    fn a_missing_replay_gate_is_refused() {
        let candidate = repeated("read", 30, INPUT, OUTPUT);
        let mut classification = classify(&candidate, ReplayEvidence::Passed, Some(1));
        classification.gates.retain(|gate| gate.name != "replay");
        let refusal = compile(&candidate, &classification, 1).unwrap_err();
        assert_eq!(refusal.reason, "replay gate missing");
    }

    #[test]
    fn the_same_input_cannot_carry_two_outputs() {
        let mut candidate = repeated("read", 30, INPUT, OUTPUT);
        let classification = classify(&candidate, ReplayEvidence::Passed, Some(1));
        let mut extra = candidate.segments[0].clone();
        extra.execution_id = "exec-extra".to_string();
        extra.observation_ids = vec!["extra".to_string()];
        extra.output_digests = vec![Some(OUTPUT_B.to_string())];
        candidate.segments.push(extra);
        candidate.deterministic = Determinism::Held;
        let refusal = compile(&candidate, &classification, 1).unwrap_err();
        assert_eq!(refusal.reason, "output conflict");
    }
}
