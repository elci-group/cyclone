//! Admit compiled currents against the journal that produced them.
//!
//! Lifecycle is independent of pressure. Shadow compares and serves the
//! original workflow. Canary serves a bounded fraction of lookups. Active
//! serves the compiled lookup until a fault. One fault rolls the current
//! back. A clean streak returns it to shadow. The interpreter runs once on
//! a compiled route and does not claim exactly-once external effects.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use cyclone_ir::{validate, CurrentIr, FALLBACK_AGENT};
use cyclone_observe::{ActionObservation, ActionOutcome, EffectClass, Trace};
use cyclone_telemetry::{Decision, DriftSignal, Telemetry, TELEMETRY_V0};
use cyclone_verify::{interpret, InterpretResult};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ADMISSION_V0: &str = "cyclone.admission/v0";
pub const POLICY_V0: &str = "cyclone.policy/v0";

const ORIGINAL: &str = FALLBACK_AGENT;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Verify(#[from] cyclone_verify::VerifyError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub enabled: bool,
    pub canary_modulus: u64,
    pub hysteresis: u64,
    pub initial: String,
}

impl Policy {
    pub fn standard() -> Self {
        Self {
            schema: POLICY_V0.to_string(),
            enabled: true,
            canary_modulus: 10,
            hysteresis: 3,
            initial: "shadow".to_string(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, RuntimeError> {
        let policy: Self = serde_json::from_str(text)
            .map_err(|error| RuntimeError::Refused(format!("policy: {error}")))?;
        policy.check()?;
        Ok(policy)
    }

    pub fn check(&self) -> Result<(), RuntimeError> {
        if self.schema != POLICY_V0 {
            return Err(refused("schema must be cyclone.policy/v0"));
        }
        if self.canary_modulus < 2 {
            return Err(refused("canary modulus must be at least 2"));
        }
        if self.hysteresis < 2 {
            return Err(refused("hysteresis must be at least 2"));
        }
        if self.initial != "shadow" {
            return Err(refused("initial lifecycle must be shadow"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentAdmission {
    pub current_id: String,
    pub tenant_id: String,
    pub pressure: String,
    pub lifecycle: String,
    pub compiled_routes: u64,
    pub original_routes: u64,
    pub rollbacks: u64,
    pub clean_streak: u64,
    pub transitions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionReport {
    pub schema: String,
    pub pass: bool,
    pub currents: Vec<CurrentAdmission>,
    pub telemetry: Telemetry,
}

pub fn admit(
    currents: &[CurrentIr],
    traces: &[Trace],
    policy: &Policy,
) -> Result<AdmissionReport, RuntimeError> {
    policy.check()?;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for current in currents {
        if !seen.insert(current.current_id.as_str()) {
            return Err(refused("repeated current"));
        }
    }

    let mut lives = Vec::with_capacity(currents.len());
    let mut drifts = Vec::new();
    for current in currents {
        let (live, drift) = Live::start(current, policy);
        if let Some(drift) = drift {
            drifts.push(drift);
        }
        lives.push(live);
    }

    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    for (offset, current) in currents.iter().enumerate() {
        index.insert(current.current_id.as_str(), offset);
    }

    let mut decisions = Vec::new();
    let mut withholding: BTreeSet<(String, String)> = BTreeSet::new();
    for ((tenant, execution), invocations) in journal(currents, traces) {
        for invocation in invocations {
            let Some(offset) = index.get(invocation.current_id.as_str()).copied() else {
                continue;
            };
            let held = withholding.contains(&(tenant.clone(), execution.clone()));
            let decision = route_one(
                &mut lives[offset],
                &currents[offset],
                policy,
                &invocation,
                held,
                &mut drifts,
            )?;
            if decision.outcome == "rolled_back" {
                withholding.insert((tenant.clone(), execution.clone()));
            }
            decisions.push(decision);
        }
    }

    let pass = controls_held(&decisions);
    let mut admitted: Vec<CurrentAdmission> = lives
        .iter()
        .zip(currents)
        .map(|(live, current)| live.finish(current))
        .collect();
    admitted.sort_by(|left, right| left.current_id.cmp(&right.current_id));
    Ok(AdmissionReport {
        schema: ADMISSION_V0.to_string(),
        pass,
        currents: admitted,
        telemetry: Telemetry {
            schema: TELEMETRY_V0.to_string(),
            decisions,
            drifts,
        },
    })
}

struct Live {
    lifecycle: String,
    streak: u64,
    canary_seen: u64,
    compiled_routes: u64,
    original_routes: u64,
    rollbacks: u64,
    transitions: Vec<String>,
    reason: String,
}

impl Live {
    fn start(current: &CurrentIr, policy: &Policy) -> (Self, Option<DriftSignal>) {
        let (lifecycle, reason, drift) = classify_start(current, policy);
        (
            Self {
                lifecycle,
                streak: 0,
                canary_seen: 0,
                compiled_routes: 0,
                original_routes: 0,
                rollbacks: 0,
                transitions: Vec::new(),
                reason,
            },
            drift,
        )
    }

    fn finish(&self, current: &CurrentIr) -> CurrentAdmission {
        CurrentAdmission {
            current_id: current.current_id.clone(),
            tenant_id: current.tenant_id.clone(),
            pressure: current.pressure.clone(),
            lifecycle: self.lifecycle.clone(),
            compiled_routes: self.compiled_routes,
            original_routes: self.original_routes,
            rollbacks: self.rollbacks,
            clean_streak: self.streak,
            transitions: self.transitions.clone(),
        }
    }
}

fn classify_start(current: &CurrentIr, policy: &Policy) -> (String, String, Option<DriftSignal>) {
    if !policy.enabled {
        return (
            "suspended".to_string(),
            "policy disabled".to_string(),
            Some(drift_signal("policy", current, "", "policy disabled")),
        );
    }
    if privileged(current) {
        return (
            "suspended".to_string(),
            "privileged effect".to_string(),
            Some(drift_signal(
                "side_effect",
                current,
                "",
                "privileged effect",
            )),
        );
    }
    if current.pressure == "gust" {
        return (
            "observed".to_string(),
            "gust stays observed".to_string(),
            None,
        );
    }
    if current.evidence.economics_minor <= 0 {
        return (
            "suspended".to_string(),
            "economics refused".to_string(),
            Some(drift_signal("economics", current, "", "economics refused")),
        );
    }
    if let Err(error) = validate(current) {
        return ("suspended".to_string(), error.to_string(), None);
    }
    ("shadow".to_string(), String::new(), None)
}

fn privileged(current: &CurrentIr) -> bool {
    let outside = |effect: &str| effect != "pure" && effect != "read_only";
    current.steps.iter().any(|step| outside(&step.effect))
        || current.side_effects.iter().any(|effect| outside(effect))
}

struct Invocation {
    execution_id: String,
    tenant_id: String,
    sequence: u64,
    current_id: String,
    input: Vec<String>,
    output: Vec<String>,
    environment: String,
    environment_fault: bool,
    digest_missing: bool,
    side_effect_fault: bool,
    tenant_fault: bool,
}

struct Segment {
    execution_id: String,
    tenant_id: String,
    sequence: u64,
    signature: Vec<String>,
    input: Vec<String>,
    output: Vec<String>,
    environment: String,
    environment_fault: bool,
    digest_missing: bool,
    side_effect_fault: bool,
    mixed_tenant: bool,
}

fn journal(
    currents: &[CurrentIr],
    traces: &[Trace],
) -> BTreeMap<(String, String), Vec<Invocation>> {
    let mut groups: BTreeMap<(String, String), Vec<Invocation>> = BTreeMap::new();
    for trace in traces {
        for segment in segments_of(trace) {
            let Some(invocation) = resolve(currents, &segment) else {
                continue;
            };
            groups
                .entry((
                    invocation.tenant_id.clone(),
                    invocation.execution_id.clone(),
                ))
                .or_default()
                .push(invocation);
        }
    }
    for group in groups.values_mut() {
        group.sort_by_key(|invocation| invocation.sequence);
    }
    groups
}

fn segments_of(trace: &Trace) -> Vec<Segment> {
    let mut observations = trace.observations.clone();
    observations.sort_by_key(|row| row.sequence);
    let mut out = Vec::new();
    let mut open: Vec<&ActionObservation> = Vec::new();
    for observation in &observations {
        if is_boundary(observation) {
            if let Some(segment) = finish_segment(&open) {
                out.push(segment);
            }
            open.clear();
            continue;
        }
        open.push(observation);
    }
    if let Some(segment) = finish_segment(&open) {
        out.push(segment);
    }
    out
}

fn finish_segment(open: &[&ActionObservation]) -> Option<Segment> {
    if open.is_empty() {
        return None;
    }
    let first = open[0];
    let mut input = Vec::new();
    let mut output = Vec::new();
    let mut environment: Option<String> = None;
    let mut environment_fault = false;
    let mut digest_missing = false;
    let mut side_effect_fault = false;
    let mut mixed_tenant = false;
    for row in open {
        if row.tenant_id != first.tenant_id || row.execution_id != first.execution_id {
            mixed_tenant = true;
        }
        if !matches!(row.effect_class, EffectClass::Pure | EffectClass::ReadOnly) {
            side_effect_fault = true;
        }
        match (
            &row.input_digest,
            &row.output_digest,
            &row.environment_digest,
        ) {
            (Some(in_digest), Some(out_digest), Some(env_digest)) => {
                input.push(in_digest.clone());
                output.push(out_digest.clone());
                match &environment {
                    None => environment = Some(env_digest.clone()),
                    Some(seen) if seen != env_digest => environment_fault = true,
                    Some(_) => {}
                }
            }
            _ => digest_missing = true,
        }
    }
    Some(Segment {
        execution_id: first.execution_id.clone(),
        tenant_id: first.tenant_id.clone(),
        sequence: first.sequence,
        signature: open.iter().map(|row| row.action_kind.clone()).collect(),
        input,
        output,
        environment: environment.unwrap_or_default(),
        environment_fault,
        digest_missing,
        side_effect_fault,
        mixed_tenant,
    })
}

fn resolve(currents: &[CurrentIr], segment: &Segment) -> Option<Invocation> {
    let exact = currents.iter().find(|current| {
        current.tenant_id == segment.tenant_id && current.signature == segment.signature
    });
    let (current, tenant_fault) = if let Some(current) = exact {
        (current, segment.mixed_tenant)
    } else {
        let mut matches = currents
            .iter()
            .filter(|current| current.signature == segment.signature);
        match (matches.next(), matches.next()) {
            (Some(current), None) => (current, true),
            _ => return None,
        }
    };
    Some(Invocation {
        execution_id: segment.execution_id.clone(),
        tenant_id: segment.tenant_id.clone(),
        sequence: segment.sequence,
        current_id: current.current_id.clone(),
        input: segment.input.clone(),
        output: segment.output.clone(),
        environment: segment.environment.clone(),
        environment_fault: segment.environment_fault,
        digest_missing: segment.digest_missing,
        side_effect_fault: segment.side_effect_fault,
        tenant_fault,
    })
}

fn is_boundary(observation: &ActionObservation) -> bool {
    observation.generative
        || observation.effect_class == EffectClass::Generative
        || observation.effect_class == EffectClass::Control
        || observation.outcome != ActionOutcome::Success
}

enum Comparison {
    Match,
    Drift { kind: &'static str, detail: String },
}

fn compare(current: &CurrentIr, invocation: &Invocation) -> Result<Comparison, RuntimeError> {
    if invocation.tenant_fault {
        return Ok(Comparison::Drift {
            kind: "tenant",
            detail: "tenant mismatch".to_string(),
        });
    }
    if invocation.side_effect_fault {
        return Ok(Comparison::Drift {
            kind: "side_effect",
            detail: "side-effect deviation".to_string(),
        });
    }
    if invocation.environment_fault {
        return Ok(Comparison::Drift {
            kind: "environment",
            detail: "environment mismatch".to_string(),
        });
    }
    if invocation.digest_missing || invocation.environment.is_empty() {
        return Ok(Comparison::Drift {
            kind: "fallback",
            detail: "digest missing".to_string(),
        });
    }
    if invocation.environment != current.environment.digest {
        return Ok(Comparison::Drift {
            kind: "environment",
            detail: "environment mismatch".to_string(),
        });
    }
    match interpret(current, &invocation.input, &invocation.environment)? {
        InterpretResult::Value { output } if output == invocation.output => Ok(Comparison::Match),
        InterpretResult::Value { .. } => Ok(Comparison::Drift {
            kind: "output",
            detail: "output mismatch".to_string(),
        }),
        InterpretResult::Fallback { reason } => Ok(Comparison::Drift {
            kind: "fallback",
            detail: reason,
        }),
    }
}

fn route_one(
    live: &mut Live,
    current: &CurrentIr,
    policy: &Policy,
    invocation: &Invocation,
    withhold: bool,
    drifts: &mut Vec<DriftSignal>,
) -> Result<Decision, RuntimeError> {
    let lifecycle = live.lifecycle.clone();
    let pressure = current.pressure.clone();
    if withhold {
        live.original_routes += 1;
        return Ok(decision(
            invocation,
            current,
            &lifecycle,
            &pressure,
            ORIGINAL,
            "withheld",
            "predecessor rolled back",
        ));
    }
    if live.lifecycle == "suspended" || live.lifecycle == "observed" {
        live.original_routes += 1;
        let reason = live.reason.clone();
        return Ok(decision(
            invocation, current, &lifecycle, &pressure, ORIGINAL, "observed", &reason,
        ));
    }
    match compare(current, invocation)? {
        Comparison::Drift {
            kind: "tenant",
            detail,
        } => {
            live.original_routes += 1;
            drifts.push(drift_signal(
                "tenant",
                current,
                &invocation.execution_id,
                &detail,
            ));
            transition(live, &lifecycle, "suspended");
            Ok(decision(
                invocation, current, &lifecycle, &pressure, ORIGINAL, "withheld", &detail,
            ))
        }
        Comparison::Drift { kind, detail } => Ok(demote(
            live, current, invocation, &lifecycle, &pressure, kind, detail, drifts,
        )),
        Comparison::Match => match lifecycle.as_str() {
            "shadow" => {
                live.original_routes += 1;
                live.streak += 1;
                let made = decision(
                    invocation,
                    current,
                    &lifecycle,
                    &pressure,
                    ORIGINAL,
                    "matched",
                    "shadow compare",
                );
                promote(live, policy, "shadow", "canary");
                Ok(made)
            }
            "canary" => {
                let sample = live.canary_seen % policy.canary_modulus == 0;
                live.canary_seen += 1;
                let made = if sample {
                    live.compiled_routes += 1;
                    decision(
                        invocation,
                        current,
                        &lifecycle,
                        &pressure,
                        "compiled",
                        "matched",
                        "canary sample",
                    )
                } else {
                    live.original_routes += 1;
                    decision(
                        invocation,
                        current,
                        &lifecycle,
                        &pressure,
                        ORIGINAL,
                        "matched",
                        "canary hold",
                    )
                };
                live.streak += 1;
                promote(live, policy, "canary", "active");
                Ok(made)
            }
            "active" => {
                live.compiled_routes += 1;
                Ok(decision(
                    invocation, current, &lifecycle, &pressure, "compiled", "matched", "active",
                ))
            }
            "rolled_back" => {
                live.original_routes += 1;
                live.streak += 1;
                let made = decision(
                    invocation, current, &lifecycle, &pressure, ORIGINAL, "matched", "recovery",
                );
                promote(live, policy, "rolled_back", "shadow");
                Ok(made)
            }
            _ => {
                live.original_routes += 1;
                Ok(decision(
                    invocation,
                    current,
                    &lifecycle,
                    &pressure,
                    ORIGINAL,
                    "observed",
                    &live.reason.clone(),
                ))
            }
        },
    }
}

fn demote(
    live: &mut Live,
    current: &CurrentIr,
    invocation: &Invocation,
    lifecycle: &str,
    pressure: &str,
    kind: &str,
    detail: String,
    drifts: &mut Vec<DriftSignal>,
) -> Decision {
    live.original_routes += 1;
    live.rollbacks += 1;
    drifts.push(drift_signal(
        kind,
        current,
        &invocation.execution_id,
        &detail,
    ));
    if live.lifecycle != "rolled_back" {
        transition(live, lifecycle, "rolled_back");
    } else {
        live.streak = 0;
    }
    decision(
        invocation,
        current,
        lifecycle,
        pressure,
        ORIGINAL,
        "rolled_back",
        &detail,
    )
}

fn promote(live: &mut Live, policy: &Policy, from: &str, to: &str) {
    if live.lifecycle == from && live.streak >= policy.hysteresis {
        transition(live, from, to);
    }
}

fn transition(live: &mut Live, from: &str, to: &str) {
    live.transitions.push(format!("{from}->{to}"));
    live.lifecycle = to.to_string();
    live.streak = 0;
    if to == "canary" {
        live.canary_seen = 0;
    }
}

fn decision(
    invocation: &Invocation,
    current: &CurrentIr,
    lifecycle: &str,
    pressure: &str,
    route: &str,
    outcome: &str,
    reason: &str,
) -> Decision {
    Decision {
        execution_id: invocation.execution_id.clone(),
        current_id: current.current_id.clone(),
        tenant_id: current.tenant_id.clone(),
        lifecycle: lifecycle.to_string(),
        pressure: pressure.to_string(),
        route: route.to_string(),
        outcome: outcome.to_string(),
        reason: reason.to_string(),
    }
}

fn drift_signal(kind: &str, current: &CurrentIr, execution_id: &str, detail: &str) -> DriftSignal {
    DriftSignal {
        kind: kind.to_string(),
        current_id: current.current_id.clone(),
        execution_id: execution_id.to_string(),
        detail: detail.to_string(),
    }
}

fn controls_held(decisions: &[Decision]) -> bool {
    decisions.iter().all(|decision| {
        let compiled = decision.route != "compiled"
            || decision.lifecycle == "canary"
            || decision.lifecycle == "active";
        let rolled = decision.outcome != "rolled_back" || decision.route == ORIGINAL;
        let held = decision.outcome != "withheld" || decision.route == ORIGINAL;
        compiled && rolled && held
    })
}

fn refused(message: &str) -> RuntimeError {
    RuntimeError::Refused(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyclone_ir::{
        Environment, Evidence, IoRow, Postcondition, Precondition, Retry, Step, CIR_V0,
        SCHEDULE_INTERPRET,
    };
    use cyclone_observe::ObservationSource;

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn current(
        tenant: &str,
        action: &str,
        effect: &str,
        pressure: &str,
        input: &str,
        output: &str,
        environment: &str,
    ) -> CurrentIr {
        let idempotency = if effect == "pure" {
            "pure"
        } else {
            "read_only"
        };
        let attempts = if effect == "pure" { 3 } else { 1 };
        let executions = if pressure == "high" { 30 } else { 15 };
        let signature = vec![action.to_string()];
        CurrentIr {
            schema: CIR_V0.to_string(),
            current_id: format!("{tenant}:{}", signature.join(",")),
            version: 1,
            tenant_id: tenant.to_string(),
            signature: signature.clone(),
            pressure: pressure.to_string(),
            steps: vec![Step {
                action_kind: action.to_string(),
                effect: effect.to_string(),
            }],
            table: vec![IoRow {
                input: vec![input.to_string()],
                output: vec![output.to_string()],
            }],
            precondition: Precondition {
                environment_eq: environment.to_string(),
                input_in_table: true,
            },
            postcondition: Postcondition {
                output_from_table: true,
                equal_digest: true,
            },
            environment: Environment {
                digest: environment.to_string(),
            },
            side_effects: vec![effect.to_string()],
            retry: Retry {
                max_attempts: attempts,
            },
            idempotency: idempotency.to_string(),
            schedule: SCHEDULE_INTERPRET.to_string(),
            fallback: ORIGINAL.to_string(),
            evidence: Evidence {
                agent_executions: executions,
                agreement_matching: executions,
                agreement_considered: executions,
                economics_minor: 1,
                observation_ids: vec!["obs".to_string()],
            },
        }
    }

    fn observation(
        tenant: &str,
        execution: &str,
        sequence: u64,
        action: &str,
        input: &str,
        output: &str,
        environment: &str,
    ) -> ActionObservation {
        let mut row = ActionObservation::new(execution, sequence, action);
        row.tenant_id = tenant.to_string();
        row.input_digest = Some(input.to_string());
        row.output_digest = Some(output.to_string());
        row.environment_digest = Some(environment.to_string());
        if action == "normalise" {
            row.effect_class = EffectClass::Pure;
            row.generative = false;
        }
        row
    }

    fn traces(
        tenant: &str,
        action: &str,
        count: usize,
        input: &str,
        output: &str,
        environment: &str,
    ) -> Vec<Trace> {
        (0..count)
            .map(|index| {
                let execution = format!("exec-{index:02}");
                let row = observation(tenant, &execution, 1, action, input, output, environment);
                Trace {
                    trace_id: execution.clone(),
                    tenant_id: tenant.to_string(),
                    execution_id: execution,
                    source: ObservationSource::Agent,
                    observations: vec![row],
                }
            })
            .collect()
    }

    fn one<'a>(report: &'a AdmissionReport, current_id: &str) -> &'a CurrentAdmission {
        report
            .currents
            .iter()
            .find(|current| current.current_id == current_id)
            .unwrap()
    }

    #[test]
    fn a_clean_journal_promotes_and_pressure_stays_put() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let high = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let low = current(
            "tenant-b",
            "pure",
            "pure",
            "low",
            &input,
            &output,
            &environment,
        );
        let mut journal = traces("tenant-a", "read", 30, &input, &output, &environment);
        journal.extend(traces("tenant-b", "pure", 8, &input, &output, &environment));
        let report = admit(&[high, low], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        assert_eq!(report.schema, "cyclone.admission/v0");
        assert_eq!(report.telemetry.schema, "cyclone.telemetry/v0");
        assert_eq!(report.telemetry.decisions.len(), 38);

        let high = one(&report, "tenant-a:read");
        assert_eq!(high.pressure, "high");
        assert_eq!(high.lifecycle, "active");
        assert_eq!(high.compiled_routes, 25);
        assert_eq!(high.original_routes, 5);
        assert_eq!(high.rollbacks, 0);
        assert!(high.transitions.contains(&"shadow->canary".to_string()));
        assert!(high.transitions.contains(&"canary->active".to_string()));

        let low = one(&report, "tenant-b:pure");
        assert_eq!(low.pressure, "low");
        assert_eq!(low.lifecycle, "active");
        assert_eq!(low.compiled_routes, 3);
        assert_eq!(low.original_routes, 5);
        assert_eq!(low.rollbacks, 0);

        let high_decisions: Vec<_> = report
            .telemetry
            .decisions
            .iter()
            .filter(|decision| decision.current_id == "tenant-a:read")
            .collect();
        assert!(high_decisions
            .iter()
            .take(3)
            .all(|decision| { decision.lifecycle == "shadow" && decision.route == ORIGINAL }));
        assert!(high_decisions.iter().all(|decision| {
            decision.route != "compiled"
                || decision.lifecycle == "canary"
                || decision.lifecycle == "active"
        }));
        for decision in &report.telemetry.decisions {
            let value = serde_json::to_value(decision).unwrap();
            let object = value.as_object().unwrap();
            assert!(object.get("prompt").is_none());
            assert!(object.get("path").is_none());
            assert!(object.get("secret").is_none());
        }
    }

    #[test]
    fn an_output_fault_on_the_first_canary_recovers_to_shadow() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let mut journal = traces("tenant-a", "read", 7, &input, &output, &environment);
        journal[3].observations[0].output_digest = Some(digest('9'));
        let report = admit(&[read], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        let admitted = one(&report, "tenant-a:read");
        assert_eq!(admitted.pressure, "high");
        assert_eq!(admitted.lifecycle, "shadow");
        assert_eq!(admitted.compiled_routes, 0);
        assert_eq!(admitted.rollbacks, 1);
        assert_eq!(
            admitted.transitions,
            vec![
                "shadow->canary".to_string(),
                "canary->rolled_back".to_string(),
                "rolled_back->shadow".to_string(),
            ]
        );
        assert!(admitted
            .transitions
            .iter()
            .all(|step| step != "canary->active"));
        let fault = report
            .telemetry
            .decisions
            .iter()
            .find(|decision| decision.outcome == "rolled_back")
            .unwrap();
        assert_eq!(fault.route, ORIGINAL);
        assert_eq!(fault.lifecycle, "canary");
        assert_eq!(report.telemetry.drifts[0].kind, "output");
        assert!(report
            .telemetry
            .decisions
            .iter()
            .all(|decision| decision.route != "compiled"));
    }

    #[test]
    fn an_environment_fault_records_environment_drift() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let mut journal = traces("tenant-a", "read", 4, &input, &output, &environment);
        journal[3].observations[0].environment_digest = Some(digest('9'));
        let report = admit(&[read], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        assert_eq!(report.telemetry.drifts[0].kind, "environment");
        let fault = &report.telemetry.decisions[3];
        assert_eq!(fault.route, ORIGINAL);
        assert_eq!(fault.outcome, "rolled_back");
        assert_eq!(one(&report, "tenant-a:read").compiled_routes, 0);
    }

    #[test]
    fn an_unknown_input_records_fallback_drift() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let mut journal = traces("tenant-a", "read", 4, &input, &output, &environment);
        journal[3].observations[0].input_digest = Some(digest('9'));
        let report = admit(&[read], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        assert_eq!(report.telemetry.drifts[0].kind, "fallback");
        assert!(report.telemetry.drifts[0].detail.contains("input"));
        assert_eq!(report.telemetry.decisions[3].route, ORIGINAL);
        assert_eq!(one(&report, "tenant-a:read").lifecycle, "rolled_back");
    }

    #[test]
    fn an_active_fault_recovers_only_to_shadow() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let mut journal = traces("tenant-a", "read", 10, &input, &output, &environment);
        journal[6].observations[0].output_digest = Some(digest('9'));
        let report = admit(&[read], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        let admitted = one(&report, "tenant-a:read");
        assert_eq!(
            admitted.transitions,
            vec![
                "shadow->canary".to_string(),
                "canary->active".to_string(),
                "active->rolled_back".to_string(),
                "rolled_back->shadow".to_string(),
            ]
        );
        assert_eq!(admitted.lifecycle, "shadow");
        assert_eq!(admitted.compiled_routes, 1);
        assert_eq!(report.telemetry.decisions[6].lifecycle, "active");
        assert_eq!(report.telemetry.decisions[6].route, ORIGINAL);
        assert_eq!(report.telemetry.decisions[6].outcome, "rolled_back");
        assert_eq!(
            admitted
                .transitions
                .iter()
                .filter(|step| step.ends_with("->active"))
                .count(),
            1
        );
    }

    #[test]
    fn a_tenant_fault_leaves_the_other_current_on_its_own_lifecycle() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let left = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let right = current(
            "tenant-b",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let mut journal = traces("tenant-a", "read", 8, &input, &output, &environment);
        journal[3].observations[0].output_digest = Some(digest('9'));
        journal.extend(traces("tenant-b", "read", 8, &input, &output, &environment));
        let report = admit(&[left, right], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        let left = one(&report, "tenant-a:read");
        let right = one(&report, "tenant-b:read");
        assert_eq!(left.rollbacks, 1);
        assert_eq!(left.compiled_routes, 0);
        assert_eq!(left.lifecycle, "shadow");
        assert_eq!(right.rollbacks, 0);
        assert_eq!(right.compiled_routes, 3);
        assert_eq!(right.original_routes, 5);
        assert_eq!(right.lifecycle, "active");
        assert_eq!(right.pressure, "high");
    }

    #[test]
    fn a_tenant_mismatch_suspends_only_that_current() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let pure = current(
            "tenant-b",
            "pure",
            "pure",
            "low",
            &input,
            &output,
            &environment,
        );
        let mut journal = traces("tenant-b", "pure", 8, &input, &output, &environment);
        let intruder = observation(
            "intruder",
            "exec-intruder",
            1,
            "read",
            &input,
            &output,
            &environment,
        );
        journal.push(Trace {
            trace_id: "exec-intruder".to_string(),
            tenant_id: "intruder".to_string(),
            execution_id: "exec-intruder".to_string(),
            source: ObservationSource::Agent,
            observations: vec![intruder],
        });
        let report = admit(&[read, pure], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        let read = one(&report, "tenant-a:read");
        assert_eq!(read.lifecycle, "suspended");
        assert_eq!(read.compiled_routes, 0);
        assert_eq!(read.transitions, vec!["shadow->suspended".to_string()]);
        assert_eq!(report.telemetry.drifts[0].kind, "tenant");
        let pure = one(&report, "tenant-b:pure");
        assert_eq!(pure.lifecycle, "active");
        assert_eq!(pure.compiled_routes, 3);
        assert_eq!(pure.rollbacks, 0);
        let breach = report
            .telemetry
            .decisions
            .iter()
            .find(|decision| decision.current_id == "tenant-a:read")
            .unwrap();
        assert_eq!(breach.outcome, "withheld");
        assert_eq!(breach.route, ORIGINAL);
    }

    #[test]
    fn a_disabled_policy_suspends_and_compiles_nothing() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let journal = traces("tenant-a", "read", 8, &input, &output, &environment);
        let mut policy = Policy::standard();
        policy.enabled = false;
        let report = admit(&[read], &journal, &policy).unwrap();
        assert!(report.pass);
        let admitted = one(&report, "tenant-a:read");
        assert_eq!(admitted.lifecycle, "suspended");
        assert_eq!(admitted.compiled_routes, 0);
        assert_eq!(admitted.original_routes, 8);
        assert!(report
            .telemetry
            .decisions
            .iter()
            .all(|decision| decision.route == ORIGINAL && decision.outcome == "observed"));
        assert_eq!(report.telemetry.drifts[0].kind, "policy");
    }

    #[test]
    fn an_initial_active_policy_is_refused() {
        let mut policy = Policy::standard();
        policy.initial = "active".to_string();
        let error = policy.check().unwrap_err();
        assert!(error.to_string().contains("shadow"));
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let journal = traces("tenant-a", "read", 1, &input, &output, &environment);
        let error = admit(&[read], &journal, &policy).unwrap_err();
        assert!(error.to_string().contains("shadow"));
    }

    #[test]
    fn a_privileged_current_never_reaches_the_interpreter() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let mut read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        read.steps[0].effect = "filesystem_mutation".to_string();
        read.side_effects = vec!["filesystem_mutation".to_string()];
        let journal = traces("tenant-a", "read", 2, &input, &output, &environment);
        let report = admit(&[read], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        let admitted = one(&report, "tenant-a:read");
        assert_eq!(admitted.lifecycle, "suspended");
        assert_eq!(admitted.compiled_routes, 0);
        assert_eq!(report.telemetry.drifts[0].kind, "side_effect");
        assert!(report
            .telemetry
            .decisions
            .iter()
            .all(|decision| decision.route == ORIGINAL));
    }

    #[test]
    fn economics_at_zero_suspends_the_current() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let mut read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        read.evidence.economics_minor = 0;
        let journal = traces("tenant-a", "read", 2, &input, &output, &environment);
        let report = admit(&[read], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        let admitted = one(&report, "tenant-a:read");
        assert_eq!(admitted.lifecycle, "suspended");
        assert_eq!(admitted.compiled_routes, 0);
        assert_eq!(report.telemetry.drifts[0].kind, "economics");
    }

    #[test]
    fn a_predecessor_rollback_withholds_the_child() {
        let environment = digest('3');
        let pure = current(
            "tenant-a",
            "pure",
            "pure",
            "high",
            &digest('1'),
            &digest('2'),
            &environment,
        );
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &digest('2'),
            &digest('4'),
            &environment,
        );
        let execution = "exec-01";
        let mut boundary = ActionObservation::new(execution, 2, "task.completed");
        boundary.tenant_id = "tenant-a".to_string();
        let trace = Trace {
            trace_id: execution.to_string(),
            tenant_id: "tenant-a".to_string(),
            execution_id: execution.to_string(),
            source: ObservationSource::Agent,
            observations: vec![
                observation(
                    "tenant-a",
                    execution,
                    1,
                    "pure",
                    &digest('1'),
                    &digest('9'),
                    &environment,
                ),
                boundary,
                observation(
                    "tenant-a",
                    execution,
                    3,
                    "read",
                    &digest('2'),
                    &digest('4'),
                    &environment,
                ),
            ],
        };
        let report = admit(&[pure, read], &[trace], &Policy::standard()).unwrap();
        assert!(report.pass);
        assert_eq!(report.telemetry.decisions.len(), 2);
        let parent = one(&report, "tenant-a:pure");
        let child = one(&report, "tenant-a:read");
        assert_eq!(parent.lifecycle, "rolled_back");
        assert_eq!(parent.rollbacks, 1);
        assert_eq!(parent.transitions, vec!["shadow->rolled_back".to_string()]);
        assert_eq!(child.lifecycle, "shadow");
        assert_eq!(child.rollbacks, 0);
        assert!(child.transitions.is_empty());
        assert_eq!(child.compiled_routes, 0);
        assert_eq!(report.telemetry.decisions[1].outcome, "withheld");
        assert_eq!(report.telemetry.decisions[1].route, ORIGINAL);
        assert_eq!(
            report.telemetry.decisions[1].reason,
            "predecessor rolled back"
        );
        assert!(report
            .telemetry
            .drifts
            .iter()
            .all(|drift| drift.current_id != "tenant-a:read"));
    }

    #[test]
    fn canary_modulus_two_holds_a_sample_on_the_original_workflow() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        let journal = traces("tenant-a", "read", 6, &input, &output, &environment);
        let mut policy = Policy::standard();
        policy.canary_modulus = 2;
        let report = admit(&[read], &journal, &policy).unwrap();
        assert!(report.pass);
        let canary: Vec<_> = report
            .telemetry
            .decisions
            .iter()
            .filter(|decision| decision.lifecycle == "canary")
            .map(|decision| decision.route.as_str())
            .collect();
        assert_eq!(canary, vec!["compiled", ORIGINAL, "compiled"]);
        assert!(canary.iter().any(|route| *route == ORIGINAL));
        assert_eq!(one(&report, "tenant-a:read").lifecycle, "active");
    }

    #[test]
    fn a_gust_stays_observed() {
        let input = digest('1');
        let output = digest('2');
        let environment = digest('3');
        let mut read = current(
            "tenant-a",
            "read",
            "read_only",
            "high",
            &input,
            &output,
            &environment,
        );
        read.pressure = "gust".to_string();
        let journal = traces("tenant-a", "read", 2, &input, &output, &environment);
        let report = admit(&[read], &journal, &Policy::standard()).unwrap();
        assert!(report.pass);
        let admitted = one(&report, "tenant-a:read");
        assert_eq!(admitted.lifecycle, "observed");
        assert_eq!(admitted.pressure, "gust");
        assert_eq!(admitted.compiled_routes, 0);
    }

    #[test]
    fn policy_parse_accepts_the_disabled_document() {
        let policy = Policy::parse(
            r#"{"schema":"cyclone.policy/v0","enabled":false,"canary_modulus":10,"hysteresis":3,"initial":"shadow"}"#,
        )
        .unwrap();
        assert!(!policy.enabled);
        assert!(Policy::parse(
            r#"{"schema":"cyclone.policy/v0","enabled":true,"canary_modulus":1,"hysteresis":3,"initial":"shadow"}"#
        )
        .is_err());
        assert!(Policy::parse(
            r#"{"schema":"cyclone.policy/v0","enabled":true,"canary_modulus":10,"hysteresis":3,"initial":"active"}"#
        )
        .is_err());
    }
}
