//! Containment programme for the twelve hostile scenarios in CTD-001 §14.
//!
//! The corpus names the scenarios. Expected outcomes live here. Property,
//! fuzz, differential, and fault probes stay inside this crate and add no
//! dependency. A passing report is journal evidence of containment.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use cyclone_compile::{compile, Refusal};
use cyclone_compose::{compose, run_graph};
use cyclone_detect::{mine, segment_traces, Candidate};
use cyclone_ir::{validate, CurrentIr, FALLBACK_AGENT};
use cyclone_observe::{ActionObservation, ActionOutcome, EffectClass, ObservationSource, Trace};
use cyclone_pressure::{classify, Classification, Pressure, Promotion, ReplayEvidence};
use cyclone_registry::{
    portfolio, Economics, EconomicsEntry, Grant, PortfolioReport, ECONOMICS_V0, GRANT_V0,
};
use cyclone_runtime::{admit, Policy};
use cyclone_verify::{interpret, shadow, InterpretResult};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ADVERSARIAL_V0: &str = "cyclone.adversarial/v0";
pub const CORPUS_V0: &str = "cyclone.adversarial.corpus/v0";

const NAMES: [&str; 12] = [
    "environment",
    "idempotency",
    "injection",
    "fan_in",
    "dependency",
    "authority",
    "partial",
    "cache",
    "cancel",
    "rare",
    "economics",
    "objectives",
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdversarialError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScenarioReport {
    pub id: u8,
    pub name: String,
    pub contained: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeReport {
    pub kind: String,
    pub cases: u64,
    pub held: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdversarialReport {
    pub schema: String,
    pub pass: bool,
    pub scenarios: Vec<ScenarioReport>,
    pub probes: Vec<ProbeReport>,
    pub contained: u64,
    pub leaked: bool,
    pub executable_effects: u64,
}

pub fn contain(corpus: &str) -> Result<AdversarialReport, AdversarialError> {
    parse_corpus(corpus)?;
    let outcomes = [
        scenario_environment(),
        scenario_idempotency(),
        scenario_injection(),
        scenario_fan_in(),
        scenario_dependency(),
        scenario_authority(),
        scenario_partial(),
        scenario_cache(),
        scenario_cancel(),
        scenario_rare(),
        scenario_economics(),
        scenario_objectives(),
    ];
    let mut scenarios = Vec::with_capacity(NAMES.len());
    for (offset, (name, outcome)) in NAMES.iter().zip(outcomes.iter()).enumerate() {
        scenarios.push(ScenarioReport {
            id: (offset as u8) + 1,
            name: (*name).to_string(),
            contained: outcome.contained,
            detail: outcome.detail.clone(),
        });
    }
    let mut probes = vec![
        probe_property(),
        probe_fuzz(),
        probe_differential(),
        probe_fault(),
    ];
    let controlled = scenarios.iter().all(|scenario| scenario.contained);
    probes.push(ProbeReport {
        kind: "controlled".to_string(),
        cases: scenarios.len() as u64,
        held: controlled,
        detail: "twelve hostile traces".to_string(),
    });
    let contained = scenarios
        .iter()
        .filter(|scenario| scenario.contained)
        .count() as u64;
    let leaked = outcomes.iter().any(|outcome| outcome.leaked);
    let executable_effects = outcomes
        .iter()
        .map(|outcome| outcome.executable_effects)
        .sum();
    let pass = contained == NAMES.len() as u64
        && probes.iter().all(|probe| probe.held)
        && !leaked
        && executable_effects == 0;
    Ok(AdversarialReport {
        schema: ADVERSARIAL_V0.to_string(),
        pass,
        scenarios,
        probes,
        contained,
        leaked,
        executable_effects,
    })
}

fn parse_corpus(text: &str) -> Result<(), AdversarialError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct File {
        schema: String,
        scenarios: Vec<String>,
    }
    let file: File = serde_json::from_str(text)
        .map_err(|error| AdversarialError::Message(format!("corpus: {error}")))?;
    if file.schema != CORPUS_V0 {
        return Err(AdversarialError::Message(
            "schema must be cyclone.adversarial.corpus/v0".to_string(),
        ));
    }
    let mut seen = BTreeSet::new();
    for name in &file.scenarios {
        if !seen.insert(name.as_str()) {
            return Err(AdversarialError::Message(format!("corpus repeats {name}")));
        }
    }
    let required: BTreeSet<&str> = NAMES.iter().copied().collect();
    if seen != required {
        return Err(AdversarialError::Message(
            "corpus scenarios must name the twelve cases".to_string(),
        ));
    }
    Ok(())
}

struct Outcome {
    contained: bool,
    detail: String,
    leaked: bool,
    executable_effects: u64,
}

fn ok_outcome(detail: &str) -> Outcome {
    Outcome {
        contained: true,
        detail: detail.to_string(),
        leaked: false,
        executable_effects: 0,
    }
}

fn bad_outcome(detail: impl Into<String>) -> Outcome {
    Outcome {
        contained: false,
        detail: detail.into(),
        leaked: false,
        executable_effects: 0,
    }
}

fn leaked_outcome(detail: impl Into<String>) -> Outcome {
    Outcome {
        contained: false,
        detail: detail.into(),
        leaked: true,
        executable_effects: 0,
    }
}

struct Assessment {
    compiled: Vec<(Candidate, CurrentIr)>,
    refusals: Vec<Refusal>,
    classes: Vec<Classification>,
    candidates: Vec<Candidate>,
}

fn assess(traces: &[Trace], economics: Option<i64>) -> Assessment {
    let mined = mine(&segment_traces(traces));
    let mut assessment = Assessment {
        compiled: Vec::new(),
        refusals: Vec::new(),
        classes: Vec::new(),
        candidates: Vec::new(),
    };
    for candidate in mined {
        let classification = classify(&candidate, ReplayEvidence::Passed, economics);
        assessment.classes.push(classification.clone());
        match compile(&candidate, &classification, economics.unwrap_or(0)) {
            Ok(current) => assessment.compiled.push((candidate.clone(), current)),
            Err(refusal) => assessment.refusals.push(refusal),
        }
        assessment.candidates.push(candidate);
    }
    assessment
}

fn scenario_environment() -> Outcome {
    let mut split = repeated("env", 30, "read", '1', '2', '3');
    for trace in split.iter_mut().skip(15) {
        trace.observations[0].environment_digest = Some(digest('7'));
    }
    let refused = assess(&split, Some(1));
    let promotion_held = refused.compiled.is_empty()
        && refused
            .classes
            .iter()
            .any(|class| class.pressure == Pressure::Gust && class.promotion == Promotion::Refused)
        && refused
            .refusals
            .iter()
            .any(|refusal| refusal.reason == "promotion refused");
    let clean = assess(&repeated("env-clean", 30, "read", '1', '2', '3'), Some(1));
    let Some((_, current)) = clean.compiled.first() else {
        return bad_outcome("clean reads produced no current");
    };
    let fallback = matches!(
        interpret(current, &[digest('1')], &digest('7')),
        Ok(InterpretResult::Fallback { reason }) if reason.contains("environment")
    );
    if promotion_held && fallback && effect_count(&clean_currents(&clean)) == 0 {
        ok_outcome("hidden environment refused promotion and fell back")
    } else {
        bad_outcome(format!(
            "environment promotion={promotion_held} fallback={fallback}"
        ))
    }
}

fn scenario_idempotency() -> Outcome {
    let assessment = assess(&repeated("write", 30, "write_file", '1', '2', '3'), Some(1));
    let held = assessment.compiled.is_empty()
        && assessment
            .classes
            .iter()
            .any(|class| class.promotion == Promotion::Eligible)
        && assessment
            .refusals
            .iter()
            .any(|refusal| refusal.reason == "effect class is not pure or read_only");
    if held {
        ok_outcome("non-idempotent effect stayed uncompiled")
    } else {
        bad_outcome("filesystem mutation compiled")
    }
}

fn scenario_injection() -> Outcome {
    let mut traces = Vec::new();
    for index in 0..30 {
        let execution = format!("inject-{index:02}");
        let mut generated = ActionObservation::new(&execution, 1, "model.response");
        generated.tenant_id = "tenant-a".to_string();
        generated.observation_id = format!("{execution}-model");
        let write = row(&execution, 2, "write_file", Some('1'), Some('2'), Some('3'));
        traces.push(trace_of(vec![generated, write]));
    }
    let segments = segment_traces(&traces);
    let generative_outside = segments.iter().all(|segment| {
        segment
            .signature
            .iter()
            .all(|part| part != "model.response")
    });
    let assessment = assess(&traces, Some(1));
    let refused = assessment.compiled.is_empty()
        && assessment
            .refusals
            .iter()
            .any(|refusal| refusal.reason == "effect class is not pure or read_only");
    if generative_outside && refused {
        ok_outcome("generative repetition stayed out of the signature")
    } else {
        bad_outcome(format!(
            "injection outside={generative_outside} refused={refused}"
        ))
    }
}

fn scenario_fan_in() -> Outcome {
    let pure = assess(&repeated("fan-pure", 30, "pure", '1', '2', '3'), Some(1));
    let mut normalise_traces = repeated("fan-norm", 30, "normalise", '5', '6', '3');
    set_effect(&mut normalise_traces, EffectClass::Pure);
    let normalise = assess(&normalise_traces, Some(1));
    let read = assess(&repeated("fan-read", 30, "read", '4', '4', '3'), Some(1));
    let (Some((_, pure_ir)), Some((_, normalise_ir)), Some((_, read_ir))) = (
        pure.compiled.first(),
        normalise.compiled.first(),
        read.compiled.first(),
    ) else {
        return bad_outcome("fan-in currents were refused");
    };
    let currents = [pure_ir.clone(), normalise_ir.clone(), read_ir.clone()];
    let left = fan_link("fan-left", "pure", '1', '2');
    let right = fan_link("fan-right", "normalise", '5', '6');
    let graph = match compose(&currents, &[left, right]) {
        Ok(graph) => graph,
        Err(error) => return bad_outcome(format!("fan-in compose {error}")),
    };
    let child = "tenant-a:read";
    let parents = graph
        .nodes
        .iter()
        .find(|node| node.node_id == child)
        .map(|node| node.parents.len())
        .unwrap_or(0);
    let waited = parents == 2
        && graph
            .waves
            .first()
            .is_some_and(|wave| wave.iter().all(|node| node != child));
    let first = match run_graph(&graph, &currents) {
        Ok(run) => run,
        Err(error) => return bad_outcome(format!("fan-in run {error}")),
    };
    let second = match run_graph(&graph, &currents) {
        Ok(run) => run,
        Err(error) => return bad_outcome(format!("fan-in rerun {error}")),
    };
    let ordered =
        first.pass && first == second && first.order.last().map(String::as_str) == Some(child);
    if waited && ordered && effect_count(&currents) == 0 {
        ok_outcome("child waited for both parents")
    } else {
        bad_outcome(format!("fan-in waited={waited} ordered={ordered}"))
    }
}

fn scenario_dependency() -> Outcome {
    let traces = repeated("dep", 30, "read", '1', '2', '3');
    let assessment = assess(&traces, Some(1));
    let Some((_, current)) = assessment.compiled.first() else {
        return bad_outcome("dependency reads produced no current");
    };
    let mut shifted = traces;
    for trace in &mut shifted {
        trace.observations[0].environment_digest = Some(digest('7'));
    }
    let admitted = match admit(std::slice::from_ref(current), &shifted, &Policy::standard()) {
        Ok(report) => report,
        Err(error) => return bad_outcome(format!("dependency admit {error}")),
    };
    let drift = admitted
        .telemetry
        .drifts
        .iter()
        .any(|drift| drift.kind == "environment");
    let original = admitted
        .telemetry
        .decisions
        .iter()
        .all(|decision| decision.route == FALLBACK_AGENT);
    let quiet = admitted
        .currents
        .iter()
        .all(|current| current.compiled_routes == 0);
    if admitted.pass
        && drift
        && original
        && quiet
        && effect_count(std::slice::from_ref(current)) == 0
    {
        ok_outcome("updated environment rolled back to the original workflow")
    } else {
        bad_outcome(format!(
            "dependency pass={} drift={drift} original={original} quiet={quiet}",
            admitted.pass
        ))
    }
}

fn scenario_authority() -> Outcome {
    let mut mixed = repeated("cross-a", 15, "read", '1', '2', '3');
    let mut foreign = repeated("cross-b", 15, "read", '1', '2', '3');
    for trace in &mut foreign {
        trace.tenant_id = "tenant-b".to_string();
        trace.observations[0].tenant_id = "tenant-b".to_string();
    }
    mixed.extend(foreign);
    let mixed_assessment = assess(&mixed, Some(1));
    let cross = mixed_assessment.candidates.len() == 2
        && mixed_assessment.candidates.iter().all(|candidate| {
            candidate.agent_executions == 15
                && candidate.agreement_considered == 15
                && candidate.segments.iter().all(|segment| {
                    segment.tenant_id == candidate.segments[0].tenant_id
                })
        })
        && mixed_assessment.compiled.iter().all(|(candidate, current)| {
            current.tenant_id == candidate.segments[0].tenant_id
        });
    let traces = paired("auth");
    let owned = assess(&traces, Some(1));
    if owned.compiled.len() != 1 {
        return bad_outcome("authority source failed to compile");
    }
    let currents = clean_currents(&owned);
    let agents = agents_of(&traces);
    let economics = read_economics(20);
    let missing = match portfolio(
        &currents,
        &agents,
        &economics,
        &["tenant-b".to_string()],
        None,
    ) {
        Ok(report) => report,
        Err(error) => return bad_outcome(format!("authority portfolio {error}")),
    };
    let grant = grant(&["read", "write_file"]);
    let escalated = match portfolio(
        &currents,
        &agents,
        &economics,
        &["tenant-b".to_string()],
        Some(&grant),
    ) {
        Ok(report) => report,
        Err(error) => return bad_outcome(format!("authority grant {error}")),
    };
    if missing.leaked || escalated.leaked {
        return leaked_outcome("authority record leaked");
    }
    let unauthorised = missing
        .refusals
        .iter()
        .any(|refusal| refusal == "authorisation missing tenant-b")
        && isolated(&missing);
    let escalation = escalated
        .refusals
        .iter()
        .any(|refusal| refusal == "authority escalation")
        && isolated(&escalated);
    if cross && unauthorised && escalation && effect_count(&currents) == 0 {
        ok_outcome("cross-tenant reuse stayed unauthorised")
    } else {
        bad_outcome(format!(
            "authority cross={cross} unauthorised={unauthorised} escalation={escalation}"
        ))
    }
}

fn scenario_partial() -> Outcome {
    let mut traces = (0..30)
        .map(|index| partial_trace(index, false))
        .collect::<Vec<_>>();
    traces.push(partial_trace(0, true));
    let assessment = assess(&traces, Some(1));
    let read = compiled_action(&assessment, "read");
    let pure = compiled_action(&assessment, "pure");
    let (Some(read), Some(pure)) = (read, pure) else {
        return bad_outcome("partial chain compiled an incomplete pair");
    };
    let currents = [read.clone(), pure.clone()];
    let absent = currents.iter().all(|current| {
        current
            .evidence
            .observation_ids
            .iter()
            .all(|id| id != "failed-pure")
    });
    let graph = match compose(&currents, &traces) {
        Ok(graph) => graph,
        Err(error) => return bad_outcome(format!("partial compose {error}")),
    };
    let run = match run_graph(&graph, &currents) {
        Ok(run) => run,
        Err(error) => return bad_outcome(format!("partial run {error}")),
    };
    if absent && run.pass && effect_count(&currents) == 0 && !graph.edges.is_empty() {
        ok_outcome("failed step stayed out of the compiled chain")
    } else {
        bad_outcome(format!(
            "partial absent={absent} pass={} edges={}",
            run.pass,
            graph.edges.len()
        ))
    }
}

fn scenario_cache() -> Outcome {
    let pure_traces = repeated("cache-pure", 30, "pure", '1', '2', '3');
    let mut normalise_traces = repeated("cache-norm", 30, "normalise", '1', '4', '3');
    set_effect(&mut normalise_traces, EffectClass::Pure);
    let pure = assess(&pure_traces, Some(1));
    let normalise = assess(&normalise_traces, Some(1));
    let (Some((_, pure_ir)), Some((_, normalise_ir))) =
        (pure.compiled.first(), normalise.compiled.first())
    else {
        return bad_outcome("cache currents were refused");
    };
    let currents = [pure_ir.clone(), normalise_ir.clone()];
    let mut traces = pure_traces;
    traces.extend(normalise_traces);
    let graph = match compose(&currents, &traces) {
        Ok(graph) => graph,
        Err(error) => return bad_outcome(format!("cache compose {error}")),
    };
    let run = match run_graph(&graph, &currents) {
        Ok(run) => run,
        Err(error) => return bad_outcome(format!("cache run {error}")),
    };
    let pure_output = interpret(pure_ir, &[digest('1')], &digest('3'));
    let normalise_output = interpret(normalise_ir, &[digest('1')], &digest('3'));
    let distinct = matches!(
        (pure_output, normalise_output),
        (
            Ok(InterpretResult::Value { output: left }),
            Ok(InterpretResult::Value { output: right })
        ) if left != right
    );
    if graph.shared.is_empty()
        && run.pass
        && run.evaluations == 2
        && distinct
        && effect_count(&currents) == 0
    {
        ok_outcome("different outputs stayed unshared")
    } else {
        bad_outcome(format!(
            "cache shared={} evaluations={} distinct={distinct}",
            graph.shared.len(),
            run.evaluations
        ))
    }
}

fn scenario_cancel() -> Outcome {
    let mut traces = Vec::new();
    for index in 0..30 {
        let execution = format!("cancel-{index:02}");
        let pure = row(&execution, 1, "pure", Some('1'), Some('2'), Some('3'));
        let boundary = control(&execution, 2, "task.completed");
        let read = row(&execution, 3, "read", Some('2'), Some('4'), Some('3'));
        traces.push(trace_of(vec![pure, boundary, read]));
    }
    let assessment = assess(&traces, Some(1));
    let pure = compiled_action(&assessment, "pure").cloned();
    let read = compiled_action(&assessment, "read").cloned();
    let (Some(pure), Some(read)) = (pure, read) else {
        return bad_outcome("cancel chain compiled an incomplete pair");
    };
    let currents = [pure, read];
    let mut graph = match compose(&currents, &traces) {
        Ok(graph) => graph,
        Err(error) => return bad_outcome(format!("cancel compose {error}")),
    };
    let child = "tenant-a:read";
    let mut armed = false;
    for edge in &mut graph.edges {
        if edge.child == child {
            edge.deadline_ms = Some(0);
            armed = true;
        }
    }
    let run = match run_graph(&graph, &currents) {
        Ok(run) => run,
        Err(error) => return bad_outcome(format!("cancel run {error}")),
    };
    let cancelled = run.cancelled == vec![child.to_string()]
        && run.order == vec!["tenant-a:pure".to_string()]
        && run.evaluations == 1
        && run.pass
        && armed;
    if cancelled && effect_count(&currents) == 0 {
        ok_outcome("zero deadline cancelled the child before interpretation")
    } else {
        bad_outcome(format!(
            "cancel armed={armed} pass={} evaluations={} order={:?} cancelled={:?}",
            run.pass, run.evaluations, run.order, run.cancelled
        ))
    }
}

fn scenario_rare() -> Outcome {
    let mut traces = repeated("rare", 29, "read", '1', '2', '3');
    traces.push(trace_of(vec![row(
        "rare-branch",
        1,
        "read",
        Some('5'),
        Some('6'),
        Some('3'),
    )]));
    let assessment = assess(&traces, Some(1));
    let Some((candidate, current)) = assessment.compiled.first() else {
        return bad_outcome("rare branch compiled nothing");
    };
    let low = current.pressure == "low"
        && assessment
            .classes
            .iter()
            .any(|class| class.pressure == Pressure::Low && class.promotion == Promotion::Eligible);
    let targeted = current.table.len() == 2
        && current
            .table
            .iter()
            .any(|row| row.output == vec![digest('6')]);
    let compared = shadow(current, candidate).ok().is_some_and(|report| {
        report.pass && report.matched == report.compared && report.compared > 0
    });
    let fallback = matches!(
        interpret(current, &[digest('8')], &digest('3')),
        Ok(InterpretResult::Fallback { reason }) if reason.contains("input")
    );
    if low
        && targeted
        && compared
        && fallback
        && assessment.compiled.len() == 1
        && effect_count(std::slice::from_ref(current)) == 0
    {
        ok_outcome("rare branch stayed low pressure and an absent input fell back")
    } else {
        bad_outcome(format!(
            "rare low={low} targeted={targeted} compared={compared} fallback={fallback} compiled={}",
            assessment.compiled.len()
        ))
    }
}

fn scenario_economics() -> Outcome {
    let traces = paired("econ");
    let zero = assess(&traces, Some(0));
    let gated = zero.compiled.is_empty()
        && zero
            .refusals
            .iter()
            .any(|refusal| refusal.reason == "promotion refused");
    let positive = assess(&traces, Some(1));
    let Some(candidate) = positive.candidates.first() else {
        return bad_outcome("economics candidate missing");
    };
    let class = classify(candidate, ReplayEvidence::Passed, Some(1));
    let erased = matches!(
        compile(candidate, &class, 0),
        Err(refusal) if refusal.reason == "economics refused"
    );
    if positive.compiled.len() != 1 {
        return bad_outcome("economics source failed to compile");
    }
    let currents = clean_currents(&positive);
    let report = match portfolio(
        &currents,
        &agents_of(&traces),
        &read_economics(400),
        &[],
        None,
    ) {
        Ok(report) => report,
        Err(error) => return bad_outcome(format!("economics portfolio {error}")),
    };
    if report.leaked {
        return leaked_outcome("economics record leaked");
    }
    let unadopted = report.benefit_minor == 0
        && report.reuses.iter().all(|reuse| !reuse.adopted)
        && report
            .refusals
            .iter()
            .any(|refusal| refusal.starts_with("economics refused"));
    if gated && erased && unadopted && effect_count(&currents) == 0 {
        ok_outcome("non-positive value stayed unadopted")
    } else {
        bad_outcome(format!(
            "economics gated={gated} erased={erased} unadopted={unadopted}"
        ))
    }
}

fn scenario_objectives() -> Outcome {
    let mut conflict = Vec::new();
    for index in 0..30 {
        let execution = format!("pay-split-{index:02}");
        let output = if index < 15 { '2' } else { '4' };
        let mut observation = row(&execution, 1, "pay", Some('1'), Some(output), Some('3'));
        observation.effect_class = EffectClass::Pure;
        conflict.push(trace_of(vec![observation]));
    }
    let split = assess(&conflict, Some(1));
    let split_held = split.compiled.is_empty()
        && split
            .refusals
            .iter()
            .any(|refusal| refusal.reason == "promotion refused");
    let mut external = Vec::new();
    for index in 0..30 {
        let execution = format!("pay-extern-{index:02}");
        let mut observation = row(&execution, 1, "pay", Some('1'), Some('2'), Some('3'));
        observation.effect_class = EffectClass::ExternalMutation;
        external.push(trace_of(vec![observation]));
    }
    let outside = assess(&external, Some(1));
    let outside_held = outside.compiled.is_empty()
        && outside
            .classes
            .iter()
            .any(|class| class.promotion == Promotion::Eligible)
        && outside
            .refusals
            .iter()
            .any(|refusal| refusal.reason == "effect class is not pure or read_only");
    if split_held && outside_held {
        ok_outcome("matching pay sequences stayed unconsolidated")
    } else {
        bad_outcome(format!(
            "objectives split={split_held} external={outside_held}"
        ))
    }
}

fn probe_property() -> ProbeReport {
    let mut cases = 0_u64;
    let mut held = true;
    let mut benign_ok = false;
    for count in [15_usize, 30] {
        for kind in ["read", "pure", "write_file", "run_checks"] {
            for economics in [None, Some(0_i64), Some(1)] {
                for env_split in [false, true] {
                    for input_split in [false, true] {
                        cases += 1;
                        let traces = grid_traces("grid", count, kind, env_split, input_split);
                        let hostile = kind == "write_file"
                            || kind == "run_checks"
                            || economics.is_none_or(|value| value <= 0)
                            || env_split;
                        let assessment = assess(&traces, economics);
                        if hostile && !assessment.compiled.is_empty() {
                            held = false;
                        }
                        if !outputs_are_safe(&assessment) {
                            held = false;
                        }
                        if kind == "read"
                            && count == 30
                            && economics == Some(1)
                            && !env_split
                            && !input_split
                        {
                            benign_ok = assessment.compiled.len() == 1
                                && assessment.compiled[0].1.pressure == "high"
                                && assessment.compiled[0].1.table.len() == 1;
                        }
                    }
                }
            }
        }
    }
    for kind in ["read", "pure"] {
        cases += 1;
        let assessment = assess(&conflict_traces(kind), Some(1));
        if !assessment.compiled.is_empty() || !outputs_are_safe(&assessment) {
            held = false;
        }
    }
    ProbeReport {
        kind: "property".to_string(),
        cases,
        held: held && benign_ok,
        detail: "grid held".to_string(),
    }
}

fn probe_fuzz() -> ProbeReport {
    let mut lcg = Lcg(0xC7C1_0E14);
    let mut held = true;
    let cases = 48_u64;
    for _ in 0..cases {
        let mut traces = repeated("fuzz", 30, "read", '1', '2', '3');
        let mut economics = Some(1_i64);
        mutate(&mut traces, lcg.next(), &mut economics);
        let assessment = assess(&traces, economics);
        if !outputs_are_safe(&assessment) {
            held = false;
        }
        let generative = traces.iter().any(|trace| {
            trace
                .observations
                .iter()
                .any(|observation| observation.generative)
        });
        if generative
            && assessment.compiled.iter().any(|(_, current)| {
                current
                    .signature
                    .iter()
                    .any(|part| part == "model.response")
            })
        {
            held = false;
        }
    }
    ProbeReport {
        kind: "fuzz".to_string(),
        cases,
        held,
        detail: "mutations held".to_string(),
    }
}

fn probe_differential() -> ProbeReport {
    let traces = repeated("diff", 30, "read", '1', '2', '3');
    let left = assess(&traces, Some(1));
    let right = assess(&traces, Some(1));
    let same = fingerprint(&left) == fingerprint(&right);
    let Some((candidate, current)) = left.compiled.first() else {
        return ProbeReport {
            kind: "differential".to_string(),
            cases: 3,
            held: false,
            detail: "differential compile produced no current".to_string(),
        };
    };
    let input = current.table[0].input.clone();
    let environment = current.environment.digest.clone();
    let once = interpret(current, &input, &environment).ok();
    let twice = interpret(current, &input, &environment).ok();
    let compared = shadow(current, candidate).ok().is_some_and(|report| {
        report.pass && report.matched == report.compared && report.compared > 0
    });
    ProbeReport {
        kind: "differential".to_string(),
        cases: 3,
        held: same && once == twice && compared,
        detail: "repeated runs matched".to_string(),
    }
}

fn probe_fault() -> ProbeReport {
    let traces = repeated("fault", 30, "read", '1', '2', '3');
    let assessment = assess(&traces, Some(1));
    let Some((_, current)) = assessment.compiled.first() else {
        return ProbeReport {
            kind: "fault".to_string(),
            cases: 3,
            held: false,
            detail: "fault compile produced no current".to_string(),
        };
    };
    let environment = matches!(
        interpret(current, &[digest('1')], &digest('7')),
        Ok(InterpretResult::Fallback { reason }) if reason.contains("environment")
    );
    let mut shifted = traces.clone();
    for trace in &mut shifted {
        trace.observations[0].environment_digest = Some(digest('7'));
    }
    let environment_admit =
        match admit(std::slice::from_ref(current), &shifted, &Policy::standard()) {
            Ok(report) => {
                report.pass
                    && report
                        .telemetry
                        .drifts
                        .iter()
                        .any(|drift| drift.kind == "environment")
                    && report
                        .telemetry
                        .decisions
                        .iter()
                        .all(|decision| decision.route == FALLBACK_AGENT)
                    && report
                        .currents
                        .iter()
                        .all(|current| current.compiled_routes == 0)
            }
            Err(_) => false,
        };
    let mut outputs = traces;
    outputs[0].observations[0].output_digest = Some(digest('9'));
    let output_admit = match admit(std::slice::from_ref(current), &outputs, &Policy::standard()) {
        Ok(report) => {
            let fault = report
                .telemetry
                .decisions
                .iter()
                .find(|decision| decision.execution_id == "fault-00");
            report.pass
                && report
                    .telemetry
                    .drifts
                    .iter()
                    .any(|drift| drift.kind == "output")
                && fault.is_some_and(|decision| {
                    decision.route == FALLBACK_AGENT && decision.outcome == "rolled_back"
                })
                && report.telemetry.decisions.iter().all(|decision| {
                    decision.outcome != "rolled_back" || decision.route == FALLBACK_AGENT
                })
        }
        Err(_) => false,
    };
    ProbeReport {
        kind: "fault".to_string(),
        cases: 3,
        held: environment && environment_admit && output_admit,
        detail: "injected faults stayed on the original workflow".to_string(),
    }
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }
}

fn mutate(traces: &mut [Trace], draw: u64, economics: &mut Option<i64>) {
    let index = (draw as usize) % traces.len();
    match draw % 8 {
        0 => traces[index].observations[0].environment_digest = Some(digest('7')),
        1 => traces[index].observations[0].output_digest = Some(digest('9')),
        2 => traces[index].observations[0].outcome = ActionOutcome::Failed,
        3 => {
            let observation = &mut traces[index].observations[0];
            observation.action_kind = "write_file".to_string();
            observation.effect_class = EffectClass::FilesystemMutation;
        }
        4 => {
            traces[index].tenant_id = "tenant-b".to_string();
            traces[index].observations[0].tenant_id = "tenant-b".to_string();
        }
        5 => *economics = Some(0),
        6 => {
            let execution = traces[index].observations[0].execution_id.clone();
            let tenant = traces[index].observations[0].tenant_id.clone();
            let mut generated = ActionObservation::new(&execution, 1, "model.response");
            generated.tenant_id = tenant;
            generated.observation_id = format!("{execution}-model");
            traces[index].observations[0].sequence = 2;
            traces[index].observations[0].observation_id = format!("{execution}-2");
            traces[index].observations.insert(0, generated);
        }
        _ => traces[index].observations[0].input_digest = None,
    }
}

fn outputs_are_safe(assessment: &Assessment) -> bool {
    assessment.compiled.iter().all(|(candidate, current)| {
        effect_count(std::slice::from_ref(current)) == 0
            && current.evidence.economics_minor > 0
            && (current.pressure != "high" || current.table.len() == 1)
            && current
                .signature
                .iter()
                .all(|part| part != "model.response")
            && validate(current).is_ok()
            && shadow(current, candidate)
                .ok()
                .is_some_and(|report| report.pass)
    })
}

fn fingerprint(assessment: &Assessment) -> String {
    let mut compiled = assessment
        .compiled
        .iter()
        .map(|(_, current)| current.current_id.clone())
        .collect::<Vec<_>>();
    compiled.sort();
    let mut refusals = assessment
        .refusals
        .iter()
        .map(|refusal| format!("{} {}", refusal.signature.join(","), refusal.reason))
        .collect::<Vec<_>>();
    refusals.sort();
    format!("{compiled:?}|{refusals:?}")
}

fn effect_count(currents: &[CurrentIr]) -> u64 {
    currents
        .iter()
        .flat_map(|current| current.steps.iter())
        .filter(|step| step.effect != "pure" && step.effect != "read_only")
        .count() as u64
}

fn clean_currents(assessment: &Assessment) -> Vec<CurrentIr> {
    assessment
        .compiled
        .iter()
        .map(|(_, current)| current.clone())
        .collect()
}

fn compiled_action<'a>(assessment: &'a Assessment, action: &str) -> Option<&'a CurrentIr> {
    assessment.compiled.iter().find_map(|(_, current)| {
        if current.signature == vec![action.to_string()] {
            Some(current)
        } else {
            None
        }
    })
}

fn isolated(report: &PortfolioReport) -> bool {
    !report.leaked
        && report
            .artifacts
            .iter()
            .all(|artifact| artifact.tenant_id == "tenant-a" && !artifact.neutral)
}

fn read_economics(detection: i64) -> Economics {
    Economics {
        schema: ECONOMICS_V0.to_string(),
        entries: vec![EconomicsEntry {
            signature: vec!["read".to_string()],
            executions: 30,
            cost_original: 10,
            cost_compiled: 1,
            cost_detection: detection,
            cost_validation: 5,
            cost_maintenance: 5,
            cost_risk: 0,
        }],
    }
}

fn grant(operations: &[&str]) -> Grant {
    Grant {
        schema: GRANT_V0.to_string(),
        source_tenant: "tenant-a".to_string(),
        consumer_tenant: "tenant-b".to_string(),
        signature: vec!["read".to_string()],
        operations: operations
            .iter()
            .map(|operation| (*operation).to_string())
            .collect(),
        principal: "adversarial-clerk".to_string(),
    }
}

fn agents_of(traces: &[Trace]) -> BTreeMap<(String, Vec<String>), BTreeSet<String>> {
    let mut index = BTreeMap::new();
    for trace in traces {
        let mut open = Vec::new();
        for observation in &trace.observations {
            if is_boundary(observation) {
                record_agents(&mut index, &open);
                open.clear();
                continue;
            }
            open.push(observation);
        }
        record_agents(&mut index, &open);
    }
    index
}

fn record_agents(
    index: &mut BTreeMap<(String, Vec<String>), BTreeSet<String>>,
    open: &[&ActionObservation],
) {
    if open.is_empty() {
        return;
    }
    let signature = open
        .iter()
        .map(|row| row.action_kind.clone())
        .collect::<Vec<_>>();
    let entry = index
        .entry((open[0].tenant_id.clone(), signature))
        .or_default();
    for row in open {
        if row.source == ObservationSource::Agent {
            entry.insert(row.agent_id.clone());
        }
    }
}

fn is_boundary(observation: &ActionObservation) -> bool {
    observation.generative
        || observation.effect_class == EffectClass::Generative
        || observation.effect_class == EffectClass::Control
        || observation.outcome != ActionOutcome::Success
}

fn digest(mark: char) -> String {
    format!("sha256:{}", mark.to_string().repeat(64))
}

fn row(
    execution: &str,
    sequence: u64,
    kind: &str,
    input: Option<char>,
    output: Option<char>,
    environment: Option<char>,
) -> ActionObservation {
    let mut observation = ActionObservation::new(execution, sequence, kind);
    observation.tenant_id = "tenant-a".to_string();
    if let (Some(input), Some(output), Some(environment)) = (input, output, environment) {
        observation =
            observation.with_digests(&digest(input), &digest(output), &digest(environment));
    }
    observation
}

fn control(execution: &str, sequence: u64, kind: &str) -> ActionObservation {
    let mut observation = ActionObservation::new(execution, sequence, kind);
    observation.tenant_id = "tenant-a".to_string();
    observation
}

fn trace_of(observations: Vec<ActionObservation>) -> Trace {
    Trace {
        trace_id: observations[0].trace_id.clone(),
        tenant_id: observations[0].tenant_id.clone(),
        execution_id: observations[0].execution_id.clone(),
        source: observations[0].source,
        observations,
    }
}

fn repeated(
    prefix: &str,
    count: usize,
    kind: &str,
    input: char,
    output: char,
    environment: char,
) -> Vec<Trace> {
    (0..count)
        .map(|index| {
            let execution = format!("{prefix}-{index:02}");
            trace_of(vec![row(
                &execution,
                1,
                kind,
                Some(input),
                Some(output),
                Some(environment),
            )])
        })
        .collect()
}

fn paired(prefix: &str) -> Vec<Trace> {
    let mut traces = repeated(prefix, 30, "read", '1', '2', '3');
    for (index, trace) in traces.iter_mut().enumerate() {
        let agent = if index < 15 { "alpha" } else { "beta" };
        trace.observations[0].agent_id = agent.to_string();
    }
    traces
}

fn set_effect(traces: &mut [Trace], effect: EffectClass) {
    for trace in traces {
        for observation in &mut trace.observations {
            observation.effect_class = effect;
        }
    }
}

fn grid_traces(
    prefix: &str,
    count: usize,
    kind: &str,
    env_split: bool,
    input_split: bool,
) -> Vec<Trace> {
    (0..count)
        .map(|index| {
            let input = if input_split && index % 2 == 1 {
                '5'
            } else {
                '1'
            };
            let output = if input_split && index % 2 == 1 {
                '6'
            } else {
                '2'
            };
            let environment = if env_split && index % 2 == 1 {
                '7'
            } else {
                '3'
            };
            let execution = format!("{prefix}-{kind}-{index:02}");
            trace_of(vec![row(
                &execution,
                1,
                kind,
                Some(input),
                Some(output),
                Some(environment),
            )])
        })
        .collect()
}

fn conflict_traces(kind: &str) -> Vec<Trace> {
    (0..30)
        .map(|index| {
            let output = if index < 15 { '2' } else { '4' };
            let execution = format!("conflict-{kind}-{index:02}");
            trace_of(vec![row(
                &execution,
                1,
                kind,
                Some('1'),
                Some(output),
                Some('3'),
            )])
        })
        .collect()
}

fn fan_link(execution: &str, parent_kind: &str, parent_in: char, parent_out: char) -> Trace {
    let mut parent = row(
        execution,
        1,
        parent_kind,
        Some(parent_in),
        Some(parent_out),
        Some('3'),
    );
    if parent_kind == "normalise" {
        parent.effect_class = EffectClass::Pure;
    }
    let boundary = control(execution, 2, "task.completed");
    let read = row(execution, 3, "read", Some('4'), Some('4'), Some('3'));
    trace_of(vec![parent, boundary, read])
}

fn partial_trace(index: usize, failed: bool) -> Trace {
    let execution = if failed {
        "partial-fail".to_string()
    } else {
        format!("partial-{index:02}")
    };
    let read = row(&execution, 1, "read", Some('1'), Some('2'), Some('3'));
    if failed {
        let mut pure = row(&execution, 2, "pure", Some('5'), Some('6'), Some('3'));
        pure.outcome = ActionOutcome::Failed;
        pure.observation_id = "failed-pure".to_string();
        return trace_of(vec![read, pure]);
    }
    let boundary = control(&execution, 2, "task.completed");
    let pure = row(&execution, 3, "pure", Some('5'), Some('6'), Some('3'));
    trace_of(vec![read, boundary, pure])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twelve_scenarios_are_contained() {
        let report = contain(include_str!("../../../traces/adversarial.json")).unwrap();
        let open: Vec<&str> = report
            .scenarios
            .iter()
            .filter(|scenario| !scenario.contained)
            .map(|scenario| scenario.detail.as_str())
            .collect();
        assert!(report.pass, "{open:?} {:?}", report.probes);
        assert_eq!(report.schema, ADVERSARIAL_V0);
        assert_eq!(report.contained, 12);
        assert!(!report.leaked);
        assert_eq!(report.executable_effects, 0);
        assert_eq!(report.scenarios.len(), 12);
        for (index, scenario) in report.scenarios.iter().enumerate() {
            assert_eq!(scenario.id, (index as u8) + 1);
            assert_eq!(scenario.name, NAMES[index]);
            assert!(scenario.contained, "{}", scenario.detail);
        }
        let kinds: Vec<&str> = report
            .probes
            .iter()
            .map(|probe| probe.kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec!["property", "fuzz", "differential", "fault", "controlled"]
        );
        for probe in &report.probes {
            assert!(probe.held, "{} {}", probe.kind, probe.detail);
        }
        assert_eq!(report.probes[0].cases, 98);
        assert_eq!(report.probes[1].cases, 48);
        assert_eq!(report.probes[2].cases, 3);
        assert_eq!(report.probes[3].cases, 3);
        assert_eq!(report.probes[4].cases, 12);
        let value = serde_json::to_value(&report).unwrap();
        assert!(value.get("prompt").is_none());
        assert!(value.get("secret").is_none());
        let again = contain(include_str!("../../../traces/adversarial.json")).unwrap();
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            serde_json::to_string(&again).unwrap()
        );
    }

    #[test]
    fn a_short_corpus_is_rejected() {
        let text = r#"{"schema":"cyclone.adversarial.corpus/v0","scenarios":["environment"]}"#;
        let error = contain(text).unwrap_err();
        assert!(error.to_string().contains("twelve"));
    }

    #[test]
    fn an_extra_scenario_is_rejected() {
        let text = r#"{"schema":"cyclone.adversarial.corpus/v0","scenarios":["environment","idempotency","injection","fan_in","dependency","authority","partial","cache","cancel","rare","economics","objectives","extra"]}"#;
        assert!(contain(text).is_err());
    }
}
