//! Audit, compile, shadow-check, schedule, admit, and portfolio a versioned trace.
//!
//! `verify` passes when every compiled current matches its source segments.
//! A trace whose candidates are all refused has nothing to compare, so the
//! report passes with a checked count of zero. `schedule` passes on the same
//! empty case. A composed run passes when every started child follows its
//! parents and the recorded effects. `admit` passes when every compiled route
//! stayed inside canary or active and every rollback stayed on the original
//! workflow. `portfolio` passes when every adopted reuse has a positive
//! shared benefit and no consumer record carries another tenant's provenance.
//! A refused candidate is left out of that journal. `adversarial` passes
//! when the twelve hostile scenarios stay contained. `dashboard` passes
//! when the projection finishes, every scope is journal or none, and the
//! portfolio record did not leak. `equivalence` passes when every declared
//! contract matches its effects and the shadow comparison held.
//! `guarantees` passes when every declared guarantee stays inside its attempt
//! limit, the shadow comparison held, and exactly-once stayed unclaimed.
//! `authority` passes when every compiled current names its tenant, principals,
//! resource, and operations, and no context elevates that workflow.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use cyclone_adversarial::{contain, AdversarialReport};
use cyclone_compile::{compile, Refusal};
use cyclone_compose::{compose, run_graph, ComposeError, CycloneGraph, ScheduleRun};
use cyclone_detect::{mine, segment_traces, Candidate};
use cyclone_ir::CurrentIr;
use cyclone_observe::{
    ingest_jsonl, ActionObservation, ActionOutcome, EffectClass, ObservationSource, ObserveError,
    Trace,
};
use cyclone_pressure::{classify, Classification, EvidenceTable, PressureError};
use cyclone_registry::{portfolio, Economics, Grant, PortfolioReport, RegistryError, PORTFOLIO_V0};
use cyclone_runtime::{admit, CurrentAdmission, Policy, RuntimeError, ADMISSION_V0};
use cyclone_telemetry::Telemetry;
use cyclone_verify::{shadow, ShadowReport, VerifyError};
use serde::Serialize;
use thiserror::Error;

pub const AUDIT_V0: &str = "cyclone.audit/v0";
pub const COMPILE_V0: &str = "cyclone.compile/v0";
pub const VERIFY_V0: &str = "cyclone.verify/v0";
pub const SCHEDULE_V0: &str = "cyclone.schedule/v0";
pub const ADMIT_V0: &str = ADMISSION_V0;
pub const RELIANCE_V0: &str = "cyclone.reliance/v0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelianceRecord {
    pub tenant_id: String,
    pub execution_id: String,
    pub task_id: String,
    pub input_digest: Option<String>,
    pub preceding_tasks: Vec<String>,
    pub reliance: Option<u8>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelianceReport {
    pub schema: String,
    pub method: String,
    pub records: Vec<RelianceRecord>,
}

/// Retrospectively measure digest-linked reliance on preceding successful
/// tasks. This is an observed association, not proof of semantic causation.
pub fn reliance_trace(text: &str) -> Result<RelianceReport, AuditError> {
    let ingested = ingest_jsonl(text)?;
    let mut records = Vec::new();
    for trace in ingested.traces {
        for (index, task) in trace.observations.iter().enumerate() {
            let linked: Vec<String> = match task.input_digest.as_deref() {
                Some(input) => trace.observations[..index].iter()
                    .filter(|prior| prior.outcome == ActionOutcome::Success
                        && prior.output_digest.as_deref() == Some(input))
                    .map(|prior| prior.observation_id.clone()).collect(),
                None => Vec::new(),
            };
            let (reliance, status) = if task.input_digest.is_none() {
                (None, "insufficient_input_evidence")
            } else if index == 0 {
                (Some(0), "no_preceding_task")
            } else {
                (Some(if linked.is_empty() { 0 } else { 100 }), "observed_digest_match")
            };
            records.push(RelianceRecord {
                tenant_id: trace.tenant_id.clone(),
                execution_id: trace.execution_id.clone(),
                task_id: task.observation_id.clone(),
                input_digest: task.input_digest.clone(),
                preceding_tasks: linked,
                reliance,
                status: status.to_string(),
            });
        }
    }
    Ok(RelianceReport {
        schema: RELIANCE_V0.to_string(),
        method: "binary digest match against prior successful outputs; association only".to_string(),
        records,
    })
}

#[derive(Debug, Error)]
pub enum AuditError {
    #[error(transparent)]
    Observe(#[from] ObserveError),
    #[error(transparent)]
    Evidence(#[from] PressureError),
    #[error(transparent)]
    Verify(#[from] VerifyError),
    #[error(transparent)]
    Compose(#[from] ComposeError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Adversarial(#[from] cyclone_adversarial::AdversarialError),
    #[error(transparent)]
    Dashboard(#[from] cyclone_dashboard::DashboardError),
    #[error(transparent)]
    Equivalence(#[from] cyclone_equivalence::EquivalenceError),
    #[error(transparent)]
    Guarantees(#[from] cyclone_guarantees::GuaranteeError),
    #[error(transparent)]
    Authority(#[from] cyclone_authority::AuthorityError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditReport {
    pub schema: String,
    pub traces: usize,
    pub observations: usize,
    pub duplicates_collapsed: u64,
    pub candidates: Vec<Classification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompileReport {
    pub schema: String,
    pub compiled: Vec<CurrentIr>,
    pub refused: Vec<Refusal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    pub schema: String,
    pub compiled: Vec<CurrentIr>,
    pub refusals: Vec<Refusal>,
    pub checked: u64,
    pub passed: u64,
    pub pass: bool,
    pub results: Vec<ShadowReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleReport {
    pub schema: String,
    pub pass: bool,
    pub currents: Vec<String>,
    pub refusals: Vec<Refusal>,
    pub graph: Option<CycloneGraph>,
    pub run: Option<ScheduleRun>,
    pub refusal: Option<String>,
}

struct Prepared {
    traces: usize,
    observations: usize,
    duplicates_collapsed: u64,
    journal: Vec<Trace>,
    mined: Vec<(Candidate, Classification, i64)>,
}

fn prepare(text: &str, evidence: &EvidenceTable) -> Result<Prepared, AuditError> {
    let ingested = ingest_jsonl(text)?;
    let segments = segment_traces(&ingested.traces);
    let mined = mine(&segments);
    let mut rows = Vec::with_capacity(mined.len());
    for candidate in mined {
        let (replay, economics) = evidence.lookup(&candidate.signature);
        let classification = classify(&candidate, replay, economics);
        rows.push((candidate, classification, economics.unwrap_or(0)));
    }
    let observations = ingested
        .traces
        .iter()
        .map(|trace| trace.observations.len())
        .sum();
    Ok(Prepared {
        traces: ingested.traces.len(),
        observations,
        duplicates_collapsed: ingested.duplicates_collapsed,
        journal: ingested.traces,
        mined: rows,
    })
}

pub fn audit(text: &str, evidence: &EvidenceTable) -> Result<AuditReport, AuditError> {
    let prepared = prepare(text, evidence)?;
    Ok(AuditReport {
        schema: AUDIT_V0.to_string(),
        traces: prepared.traces,
        observations: prepared.observations,
        duplicates_collapsed: prepared.duplicates_collapsed,
        candidates: prepared
            .mined
            .into_iter()
            .map(|(_, classification, _)| classification)
            .collect(),
    })
}

pub fn compile_trace(text: &str, evidence: &EvidenceTable) -> Result<CompileReport, AuditError> {
    let prepared = prepare(text, evidence)?;
    let mut compiled = Vec::new();
    let mut refused = Vec::new();
    for (candidate, classification, economics) in prepared.mined {
        match compile(&candidate, &classification, economics) {
            Ok(current) => compiled.push(current),
            Err(refusal) => refused.push(refusal),
        }
    }
    Ok(CompileReport {
        schema: COMPILE_V0.to_string(),
        compiled,
        refused,
    })
}

pub fn verify_trace(text: &str, evidence: &EvidenceTable) -> Result<VerifyReport, AuditError> {
    let prepared = prepare(text, evidence)?;
    let mut compiled = Vec::new();
    let mut refusals = Vec::new();
    let mut results = Vec::new();
    let mut checked = 0_u64;
    let mut passed = 0_u64;
    for (candidate, classification, economics) in prepared.mined {
        match compile(&candidate, &classification, economics) {
            Ok(current) => {
                let report = shadow(&current, &candidate)?;
                checked += report.compared;
                passed += report.matched;
                results.push(report);
                compiled.push(current);
            }
            Err(refusal) => refusals.push(refusal),
        }
    }
    let pass = results.iter().all(|result| result.pass);
    Ok(VerifyReport {
        schema: VERIFY_V0.to_string(),
        compiled,
        refusals,
        checked,
        passed,
        pass,
        results,
    })
}

pub fn schedule_trace(text: &str, evidence: &EvidenceTable) -> Result<ScheduleReport, AuditError> {
    let prepared = prepare(text, evidence)?;
    let mut compiled = Vec::new();
    let mut refusals = Vec::new();
    for (candidate, classification, economics) in prepared.mined {
        match compile(&candidate, &classification, economics) {
            Ok(current) => compiled.push(current),
            Err(refusal) => refusals.push(refusal),
        }
    }
    let currents = compiled
        .iter()
        .map(|current| current.current_id.clone())
        .collect();
    if compiled.is_empty() || !refusals.is_empty() {
        return Ok(ScheduleReport {
            schema: SCHEDULE_V0.to_string(),
            pass: true,
            currents,
            refusals,
            graph: None,
            run: None,
            refusal: None,
        });
    }
    match compose(&compiled, &prepared.journal) {
        Ok(graph) => {
            let run = run_graph(&graph, &compiled)?;
            let pass = run.pass;
            Ok(ScheduleReport {
                schema: SCHEDULE_V0.to_string(),
                pass,
                currents,
                refusals,
                graph: Some(graph),
                run: Some(run),
                refusal: None,
            })
        }
        Err(error) => Ok(ScheduleReport {
            schema: SCHEDULE_V0.to_string(),
            pass: true,
            currents,
            refusals,
            graph: None,
            run: None,
            refusal: Some(error.to_string()),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmitReport {
    pub schema: String,
    pub pass: bool,
    pub currents: Vec<CurrentAdmission>,
    pub telemetry: Telemetry,
    pub refusals: Vec<Refusal>,
}

pub fn admit_trace(
    text: &str,
    evidence: &EvidenceTable,
    policy: &Policy,
) -> Result<AdmitReport, AuditError> {
    let prepared = prepare(text, evidence)?;
    let mut compiled = Vec::new();
    let mut refusals = Vec::new();
    for (candidate, classification, economics) in prepared.mined {
        match compile(&candidate, &classification, economics) {
            Ok(current) => compiled.push(current),
            Err(refusal) => refusals.push(refusal),
        }
    }
    if compiled.is_empty() || !refusals.is_empty() {
        return Ok(AdmitReport {
            schema: ADMIT_V0.to_string(),
            pass: true,
            currents: Vec::new(),
            telemetry: Telemetry::empty(),
            refusals,
        });
    }
    let admitted = admit(&compiled, &prepared.journal, policy)?;
    Ok(AdmitReport {
        schema: admitted.schema,
        pass: admitted.pass,
        currents: admitted.currents,
        telemetry: admitted.telemetry,
        refusals,
    })
}

pub fn portfolio_trace(
    text: &str,
    evidence: &EvidenceTable,
    economics: &Economics,
    grant: Option<&Grant>,
) -> Result<PortfolioReport, AuditError> {
    let prepared = prepare(text, evidence)?;
    let mut compiled = Vec::new();
    let mut refusals = Vec::new();
    for (candidate, classification, economics_minor) in prepared.mined {
        match compile(&candidate, &classification, economics_minor) {
            Ok(current) => compiled.push(current),
            Err(refusal) => refusals.push(format!(
                "{} {}",
                refusal.signature.join(","),
                refusal.reason
            )),
        }
    }
    if compiled.is_empty() || !refusals.is_empty() {
        return Ok(PortfolioReport {
            schema: PORTFOLIO_V0.to_string(),
            pass: true,
            patterns: Vec::new(),
            artifacts: Vec::new(),
            reuses: Vec::new(),
            refusals,
            benefit_minor: 0,
            leaked: false,
        });
    }
    let consumers = grant
        .map(|grant| vec![grant.consumer_tenant.clone()])
        .unwrap_or_default();
    let mut report = portfolio(
        &compiled,
        &agent_index(&prepared.journal),
        economics,
        &consumers,
        grant,
    )?;
    report.refusals.extend(refusals);
    report.refusals.sort();
    Ok(report)
}

fn agent_index(traces: &[Trace]) -> BTreeMap<(String, Vec<String>), BTreeSet<String>> {
    let mut index = BTreeMap::new();
    for trace in traces {
        let mut observations = trace.observations.clone();
        observations.sort_by_key(|row| row.sequence);
        let mut open = Vec::new();
        for observation in observations {
            if is_boundary(&observation) {
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
    open: &[ActionObservation],
) {
    if open.is_empty() {
        return;
    }
    let signature: Vec<String> = open.iter().map(|row| row.action_kind.clone()).collect();
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

pub fn adversarial_trace(text: &str) -> Result<AdversarialReport, AuditError> {
    Ok(contain(text)?)
}

pub fn dashboard_trace(
    text: &str,
    evidence: &EvidenceTable,
    policy: &Policy,
    economics: &Economics,
    grant: Option<&Grant>,
) -> Result<cyclone_dashboard::DashboardReport, AuditError> {
    let audited = audit(text, evidence)?;
    let compiled = compile_trace(text, evidence)?;
    let verified = verify_trace(text, evidence)?;
    let scheduled = schedule_trace(text, evidence)?;
    let admitted = admit_trace(text, evidence, policy)?;
    let ported = portfolio_trace(text, evidence, economics, grant)?;
    let ingested = ingest_jsonl(text)?;
    let mut currents = Vec::new();
    for current in &compiled.compiled {
        let bytes = serde_json::to_vec(current).map_err(|error| {
            AuditError::Dashboard(cyclone_dashboard::DashboardError::Message(format!(
                "encode {error}"
            )))
        })?;
        currents.push(cyclone_dashboard::CurrentView {
            current_id: current.current_id.clone(),
            version: current.version,
            tenant_id: current.tenant_id.clone(),
            signature: current.signature.clone(),
            schedule: current.schedule.clone(),
            effects: current.side_effects.clone(),
            artifact_sha256: cyclone_dashboard::artifact_sha256(&bytes),
        });
    }
    let graph = match scheduled.graph {
        Some(graph) => cyclone_dashboard::GraphView {
            graph_id: graph.graph_id,
            nodes: graph
                .nodes
                .into_iter()
                .map(|node| cyclone_dashboard::GraphNode {
                    node_id: node.node_id,
                    signature: node.signature,
                    parents: node.parents,
                })
                .collect(),
            edges: graph
                .edges
                .into_iter()
                .map(|edge| cyclone_dashboard::GraphEdge {
                    parent: edge.parent,
                    child: edge.child,
                    trigger: edge.trigger,
                })
                .collect(),
            critical_path: graph.critical_path,
        },
        None => cyclone_dashboard::GraphView::empty(),
    };
    let mut actions = Vec::new();
    for trace in &ingested.traces {
        for observation in &trace.observations {
            actions.push(cyclone_dashboard::ActionRecord {
                tenant_id: observation.tenant_id.clone(),
                execution_id: observation.execution_id.clone(),
                sequence: observation.sequence,
                action_kind: observation.action_kind.clone(),
                outcome: outcome_name(observation.outcome).to_string(),
            });
        }
    }
    let source = cyclone_dashboard::DashboardSource {
        candidates: audited
            .candidates
            .iter()
            .map(|candidate| {
                let (_, economics_minor) = evidence.lookup(&candidate.signature);
                cyclone_dashboard::CandidateView {
                    owner: owners_for(&candidate.signature, &ingested.traces),
                    signature: candidate.signature.clone(),
                    pressure: pressure_name(candidate).to_string(),
                    promotion: promotion_name(candidate).to_string(),
                    agent_executions: candidate.agent_executions,
                    observations: candidate.observation_ids.len() as u64,
                    economics_minor: economics_minor.unwrap_or(0),
                    agreement_matching: candidate.agreement_matching,
                    agreement_considered: candidate.agreement_considered,
                    explanation: candidate.explanation.clone(),
                }
            })
            .collect(),
        currents,
        shadows: verified
            .results
            .into_iter()
            .map(|result| cyclone_dashboard::ShadowView {
                current_id: result.current_id,
                checked: result.compared,
                passed: result.matched,
                pass: result.pass,
            })
            .collect(),
        graph,
        admissions: admitted
            .currents
            .into_iter()
            .map(|current| cyclone_dashboard::AdmissionView {
                current_id: current.current_id,
                lifecycle: current.lifecycle,
                transitions: current.transitions,
            })
            .collect(),
        drifts: admitted
            .telemetry
            .drifts
            .into_iter()
            .map(|drift| cyclone_dashboard::DriftView {
                kind: drift.kind,
                current_id: drift.current_id,
                detail: drift.detail,
            })
            .collect(),
        decisions: admitted
            .telemetry
            .decisions
            .into_iter()
            .map(|decision| cyclone_dashboard::DecisionView {
                execution_id: decision.execution_id,
                current_id: decision.current_id,
                route: decision.route,
                outcome: decision.outcome,
            })
            .collect(),
        actions,
        economics: economics
            .entries
            .iter()
            .map(|entry| cyclone_dashboard::EconomicsView {
                signature: entry.signature.clone(),
                executions: entry.executions,
                cost_original: entry.cost_original,
                cost_compiled: entry.cost_compiled,
                cost_detection: entry.cost_detection,
                cost_validation: entry.cost_validation,
                cost_maintenance: entry.cost_maintenance,
                cost_risk: entry.cost_risk,
            })
            .collect(),
        portfolio_benefit_minor: ported.benefit_minor,
        portfolio_leaked: ported.leaked,
        portfolio_refusals: ported.refusals,
        compile_refusals: compiled
            .refused
            .into_iter()
            .map(|refusal| cyclone_dashboard::RefusalView {
                signature: refusal.signature,
                reason: refusal.reason,
            })
            .collect(),
    };
    Ok(cyclone_dashboard::project(&source)?)
}

pub fn equivalence_trace(
    text: &str,
    evidence: &EvidenceTable,
) -> Result<cyclone_equivalence::EquivalenceReport, AuditError> {
    let compiled = compile_trace(text, evidence)?;
    let verified = verify_trace(text, evidence)?;
    let mut shadows = BTreeMap::new();
    for result in verified.results {
        shadows.insert(result.current_id.clone(), result);
    }
    let currents = compiled
        .compiled
        .iter()
        .map(|current| {
            let shadow = shadows.get(&current.current_id);
            cyclone_equivalence::CurrentClaim {
                current_id: current.current_id.clone(),
                signature: current.signature.clone(),
                effects: current.side_effects.clone(),
                shadow_pass: shadow.is_some_and(|row| row.pass),
                shadow_checked: shadow.map(|row| row.compared).unwrap_or(0),
                shadow_matched: shadow.map(|row| row.matched).unwrap_or(0),
            }
        })
        .collect();
    let source = cyclone_equivalence::EquivalenceSource {
        currents,
        refusals: compiled
            .refused
            .into_iter()
            .map(|refusal| cyclone_equivalence::RefusalClaim {
                signature: refusal.signature,
                reason: refusal.reason,
            })
            .collect(),
    };
    Ok(cyclone_equivalence::declare(&source)?)
}

pub fn guarantees_trace(
    text: &str,
    evidence: &EvidenceTable,
) -> Result<cyclone_guarantees::GuaranteeReport, AuditError> {
    let compiled = compile_trace(text, evidence)?;
    let verified = verify_trace(text, evidence)?;
    let mut shadows = BTreeMap::new();
    for result in verified.results {
        shadows.insert(result.current_id.clone(), result);
    }
    let currents = compiled
        .compiled
        .iter()
        .map(|current| {
            let shadow = shadows.get(&current.current_id);
            cyclone_guarantees::CurrentClaim {
                current_id: current.current_id.clone(),
                signature: current.signature.clone(),
                effects: current.side_effects.clone(),
                idempotency: current.idempotency.clone(),
                max_attempts: current.retry.max_attempts,
                shadow_pass: shadow.is_some_and(|row| row.pass),
                shadow_checked: shadow.map(|row| row.compared).unwrap_or(0),
                shadow_matched: shadow.map(|row| row.matched).unwrap_or(0),
            }
        })
        .collect();
    let source = cyclone_guarantees::GuaranteeSource {
        currents,
        refusals: compiled
            .refused
            .into_iter()
            .map(|refusal| cyclone_guarantees::RefusalClaim {
                signature: refusal.signature,
                reason: refusal.reason,
            })
            .collect(),
    };
    Ok(cyclone_guarantees::declare(&source)?)
}

pub fn authority_trace(
    text: &str,
    evidence: &EvidenceTable,
    grant: Option<&Grant>,
) -> Result<cyclone_authority::AuthorityReport, AuditError> {
    let compiled = compile_trace(text, evidence)?;
    let ingested = ingest_jsonl(text)?;
    let agents = agent_index(&ingested.traces);
    let currents = compiled
        .compiled
        .iter()
        .map(|current| {
            let key = (current.tenant_id.clone(), current.signature.clone());
            let names = agents.get(&key).cloned().unwrap_or_default();
            cyclone_authority::CurrentClaim {
                current_id: current.current_id.clone(),
                tenant_id: current.tenant_id.clone(),
                signature: current.signature.clone(),
                effects: current.side_effects.clone(),
                environment: current.environment.digest.clone(),
                agents: names.into_iter().collect(),
            }
        })
        .collect();
    let source = cyclone_authority::AuthoritySource {
        currents,
        refusals: compiled
            .refused
            .into_iter()
            .map(|refusal| cyclone_authority::RefusalClaim {
                signature: refusal.signature,
                reason: refusal.reason,
            })
            .collect(),
        grant: grant.map(|grant| cyclone_authority::GrantClaim {
            source_tenant: grant.source_tenant.clone(),
            consumer_tenant: grant.consumer_tenant.clone(),
            principal: grant.principal.clone(),
            signature: grant.signature.clone(),
            operations: grant.operations.clone(),
        }),
    };
    Ok(cyclone_authority::declare(&source)?)
}

fn owners_for(signature: &[String], traces: &[Trace]) -> String {
    let mut tenants = BTreeSet::new();
    for trace in traces {
        let mut open: Vec<&ActionObservation> = Vec::new();
        for observation in &trace.observations {
            if is_boundary(observation) {
                record_owner(&mut tenants, signature, &open);
                open.clear();
                continue;
            }
            open.push(observation);
        }
        record_owner(&mut tenants, signature, &open);
    }
    tenants.into_iter().collect::<Vec<_>>().join(",")
}

fn record_owner(tenants: &mut BTreeSet<String>, signature: &[String], open: &[&ActionObservation]) {
    if open.is_empty() {
        return;
    }
    let found: Vec<&str> = open.iter().map(|row| row.action_kind.as_str()).collect();
    let wanted: Vec<&str> = signature.iter().map(String::as_str).collect();
    if found == wanted {
        tenants.insert(open[0].tenant_id.clone());
    }
}

fn outcome_name(outcome: ActionOutcome) -> &'static str {
    match outcome {
        ActionOutcome::Success => "success",
        ActionOutcome::Rejected => "rejected",
        ActionOutcome::Failed => "failed",
        ActionOutcome::Denied => "denied",
        ActionOutcome::Unclassified => "unclassified",
    }
}

pub fn render_adversarial(report: &AdversarialReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("contained: {}\n", report.contained));
    out.push_str(&format!("leaked: {}\n", report.leaked));
    out.push_str(&format!(
        "executable_effects: {}\n",
        report.executable_effects
    ));
    for scenario in &report.scenarios {
        out.push_str(&format!(
            "scenario: {} {} contained={} {}\n",
            scenario.id, scenario.name, scenario.contained, scenario.detail
        ));
    }
    for probe in &report.probes {
        out.push_str(&format!(
            "probe: {} cases={} held={} {}\n",
            probe.kind, probe.cases, probe.held, probe.detail
        ));
    }
    out
}

pub fn render_dashboard(report: &cyclone_dashboard::DashboardReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("record_scope: {}\n", report.record_scope));
    out.push_str(&format!("registry: {}\n", report.registry.len()));
    for row in &report.registry {
        let version = match row.version {
            Some(version) => version.to_string(),
            None => "none".to_string(),
        };
        out.push_str(&format!(
            "current: {} pressure={} version={} owner={} status={} scope={}\n",
            row.signature.join(","),
            row.pressure,
            version,
            row.owner,
            row.status,
            row.scope
        ));
    }
    out.push_str(&format!(
        "critical_path: {}\n",
        report.graph.critical_path.join(",")
    ));
    out.push_str(&format!("edges: {}\n", report.graph.edges.len()));
    out.push_str(&format!(
        "avoided_inference: {}\n",
        report.economics.avoided_inference
    ));
    out.push_str(&format!(
        "benefit_minor: {}\n",
        report.economics.benefit_minor
    ));
    out.push_str(&format!("leaked: {}\n", report.safety.leaked));
    out.push_str(&format!(
        "rejected: {}\n",
        report.safety.rejected_promotions.len()
    ));
    out.push_str(&format!("suspended: {}\n", report.safety.suspended.len()));
    out
}

pub fn render_dashboard_html(report: &cyclone_dashboard::DashboardReport) -> String {
    cyclone_dashboard::render_html(report)
}

pub fn render_equivalence(report: &cyclone_equivalence::EquivalenceReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("contracts: {}\n", report.contracts.len()));
    for contract in &report.contracts {
        out.push_str(&format!(
            "contract: {} category={} check={} tolerance={} shadow={}/{} irreversible_replay={}\n",
            contract.current_id,
            contract.category,
            contract.check,
            contract.tolerance,
            contract.shadow_matched,
            contract.shadow_checked,
            contract.irreversible_replay
        ));
    }
    out.push_str(&format!("undeclared: {}\n", report.undeclared.len()));
    for row in &report.undeclared {
        out.push_str(&format!(
            "undeclared: {} {}\n",
            row.signature.join(","),
            row.reason
        ));
    }
    out
}

pub fn render_guarantees(report: &cyclone_guarantees::GuaranteeReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("exactly_once: {}\n", report.exactly_once));
    out.push_str(&format!("transactional: {}\n", report.transactional));
    out.push_str(&format!("guarantees: {}\n", report.guarantees.len()));
    for row in &report.guarantees {
        out.push_str(&format!(
            "guarantee: {} category={} kind={} attempts={}/{} exactly_once={} journal={}\n",
            row.current_id,
            row.category,
            row.kind,
            row.attempts_performed,
            row.attempts_limit,
            row.exactly_once,
            row.journal
        ));
    }
    out.push_str(&format!("unclaimed: {}\n", report.unclaimed.len()));
    for row in &report.unclaimed {
        out.push_str(&format!(
            "unclaimed: {} {} exactly_once={}\n",
            row.signature.join(","),
            row.reason,
            row.exactly_once
        ));
    }
    out
}

pub fn render_authority(report: &cyclone_authority::AuthorityReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("elevated: {}\n", report.elevated));
    out.push_str(&format!(
        "historical_permission: {}\n",
        report.historical_permission
    ));
    out.push_str(&format!("contexts: {}\n", report.contexts.len()));
    for row in &report.contexts {
        out.push_str(&format!(
            "context: {} binding={} tenant={} principals={} operations={} resources={} elevated={} historical_permission={}\n",
            row.current_id,
            row.binding,
            row.tenant_id,
            row.principals.join(","),
            row.operations.join(","),
            row.resources.len(),
            row.elevated,
            row.historical_permission
        ));
    }
    out.push_str(&format!("unauthorised: {}\n", report.unauthorised.len()));
    for row in &report.unauthorised {
        out.push_str(&format!(
            "unauthorised: {} {} elevated={} historical_permission={}\n",
            row.signature.join(","),
            row.reason,
            row.elevated,
            row.historical_permission
        ));
    }
    out
}

pub fn render_portfolio(report: &PortfolioReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("patterns: {}\n", report.patterns.len()));
    out.push_str(&format!("benefit_minor: {}\n", report.benefit_minor));
    out.push_str(&format!("leaked: {}\n", report.leaked));
    for pattern in &report.patterns {
        out.push_str(&format!(
            "pattern: {} {} agents={}\n",
            pattern.tenant_id,
            pattern.signature.join(","),
            pattern.agents.join(",")
        ));
    }
    for artifact in &report.artifacts {
        out.push_str(&format!(
            "artifact: {} tenant={} neutral={}\n",
            artifact.artifact_id, artifact.tenant_id, artifact.neutral
        ));
    }
    for reuse in &report.reuses {
        out.push_str(&format!(
            "reuse: {} adopted={} benefit={} {}\n",
            reuse.consumer_tenant, reuse.adopted, reuse.benefit_minor, reuse.reason
        ));
    }
    for refusal in &report.refusals {
        out.push_str(&format!("refusal: {refusal}\n"));
    }
    out
}

pub fn render_text(report: &AuditReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("traces: {}\n", report.traces));
    out.push_str(&format!("observations: {}\n", report.observations));
    out.push_str(&format!(
        "duplicates_collapsed: {}\n",
        report.duplicates_collapsed
    ));
    out.push_str(&format!("candidates: {}\n", report.candidates.len()));
    for candidate in &report.candidates {
        out.push('\n');
        out.push_str(&format!("signature: {}\n", candidate.signature.join(",")));
        out.push_str(&format!("pressure: {}\n", pressure_name(candidate)));
        out.push_str(&format!("promotion: {}\n", promotion_name(candidate)));
        out.push_str(&format!(
            "agent_executions: {}\n",
            candidate.agent_executions
        ));
        out.push_str(&format!(
            "mechanical_executions: {}\n",
            candidate.mechanical_executions
        ));
        for gate in &candidate.gates {
            out.push_str(&format!(
                "gate {}: high={} low={} observed={} high_requires={} low_requires={}\n",
                gate.name,
                gate.high_pass,
                gate.low_pass,
                gate.observed,
                gate.high_requirement,
                gate.low_requirement
            ));
        }
        for line in &candidate.explanation {
            out.push_str(&format!("explanation: {line}\n"));
        }
    }
    out
}

pub fn render_compile(report: &CompileReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("compiled: {}\n", report.compiled.len()));
    out.push_str(&format!("refused: {}\n", report.refused.len()));
    for current in &report.compiled {
        out.push('\n');
        out.push_str(&format!("current: {}\n", current.current_id));
        out.push_str(&format!("pressure: {}\n", current.pressure));
        out.push_str(&format!("signature: {}\n", current.signature.join(",")));
        out.push_str(&format!("schedule: {}\n", current.schedule));
        out.push_str(&format!("idempotency: {}\n", current.idempotency));
        out.push_str(&format!("rows: {}\n", current.table.len()));
    }
    for refusal in &report.refused {
        out.push_str(&format!(
            "refusal: {} {}\n",
            refusal.signature.join(","),
            refusal.reason
        ));
    }
    out
}

pub fn render_verify(report: &VerifyReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("checked: {}\n", report.checked));
    out.push_str(&format!("passed: {}\n", report.passed));
    out.push_str(&format!("compiled: {}\n", report.compiled.len()));
    out.push_str(&format!("refusals: {}\n", report.refusals.len()));
    for result in &report.results {
        out.push_str(&format!(
            "shadow: {} compared={} matched={} mismatched={} pass={}\n",
            result.current_id, result.compared, result.matched, result.mismatched, result.pass
        ));
    }
    for refusal in &report.refusals {
        out.push_str(&format!(
            "refusal: {} {}\n",
            refusal.signature.join(","),
            refusal.reason
        ));
    }
    out
}

pub fn render_schedule(report: &ScheduleReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("currents: {}\n", report.currents.len()));
    out.push_str(&format!("refusals: {}\n", report.refusals.len()));
    if let Some(graph) = &report.graph {
        out.push_str(&format!("graph: {}\n", graph.graph_id));
        out.push_str(&format!("nodes: {}\n", graph.nodes.len()));
        out.push_str(&format!("edges: {}\n", graph.edges.len()));
        out.push_str(&format!("waves: {}\n", graph.waves.len()));
        out.push_str(&format!(
            "critical_path: {}\n",
            graph.critical_path.join(",")
        ));
        for edge in &graph.edges {
            out.push_str(&format!(
                "edge: {} -> {} {}\n",
                edge.parent, edge.child, edge.trigger
            ));
        }
    }
    if let Some(run) = &report.run {
        out.push_str(&format!("order: {}\n", run.order.join(",")));
        out.push_str(&format!("effects: {}\n", run.effects.join(",")));
        out.push_str(&format!("evaluations: {}\n", run.evaluations));
        out.push_str(&format!("reuses: {}\n", run.reuses));
    }
    if let Some(refusal) = &report.refusal {
        out.push_str(&format!("refusal: {refusal}\n"));
    }
    for refusal in &report.refusals {
        out.push_str(&format!(
            "refusal: {} {}\n",
            refusal.signature.join(","),
            refusal.reason
        ));
    }
    out
}

pub fn render_admit(report: &AdmitReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", report.schema));
    out.push_str(&format!("pass: {}\n", report.pass));
    out.push_str(&format!("currents: {}\n", report.currents.len()));
    for current in &report.currents {
        out.push_str(&format!("current: {}\n", current.current_id));
        out.push_str(&format!("lifecycle: {}\n", current.lifecycle));
        out.push_str(&format!("pressure: {}\n", current.pressure));
        out.push_str(&format!("compiled_routes: {}\n", current.compiled_routes));
        out.push_str(&format!("original_routes: {}\n", current.original_routes));
        out.push_str(&format!("rollbacks: {}\n", current.rollbacks));
        out.push_str(&format!("transitions: {}\n", current.transitions.join(",")));
    }
    out.push_str(&format!("telemetry: {}\n", report.telemetry.schema));
    out.push_str(&format!(
        "decisions: {}\n",
        report.telemetry.decisions.len()
    ));
    out.push_str(&format!("drifts: {}\n", report.telemetry.drifts.len()));
    for refusal in &report.refusals {
        out.push_str(&format!(
            "refusal: {} {}\n",
            refusal.signature.join(","),
            refusal.reason
        ));
    }
    out
}

fn pressure_name(candidate: &Classification) -> &'static str {
    match candidate.pressure {
        cyclone_pressure::Pressure::High => "high",
        cyclone_pressure::Pressure::Low => "low",
        cyclone_pressure::Pressure::Gust => "gust",
    }
}

fn promotion_name(candidate: &Classification) -> &'static str {
    match candidate.promotion {
        cyclone_pressure::Promotion::Eligible => "eligible",
        cyclone_pressure::Promotion::Refused => "refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_without_evidence_compile_to_a_refusal() {
        let report = compile_trace(
            include_str!("../../../traces/reads-30.jsonl"),
            &EvidenceTable::empty(),
        )
        .unwrap();
        assert!(report.compiled.is_empty());
        assert_eq!(report.refused.len(), 1);
        assert_eq!(report.refused[0].reason, "promotion refused");
        let verified = verify_trace(
            include_str!("../../../traces/reads-30.jsonl"),
            &EvidenceTable::empty(),
        )
        .unwrap();
        assert!(verified.pass);
        assert_eq!(verified.checked, 0);
    }

    #[test]
    fn a_split_environment_stays_uncompiled() {
        let evidence =
            EvidenceTable::parse(include_str!("../../../traces/reads-30.evidence.json")).unwrap();
        let report = compile_trace(
            include_str!("../../../traces/environment-split.jsonl"),
            &evidence,
        )
        .unwrap();
        assert!(report.compiled.is_empty());
        assert_eq!(report.refused[0].reason, "promotion refused");
    }

    #[test]
    fn the_lucid_fixture_verifies_with_nothing_checked() {
        let report = verify_trace(
            include_str!("../../../traces/lucid-session-v1.jsonl"),
            &EvidenceTable::empty(),
        )
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.checked, 0);
        assert!(report.compiled.is_empty());
        assert!(report.refusals.is_empty());
    }
}
