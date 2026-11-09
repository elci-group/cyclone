//! Operator projection of a cyclone journal.
//!
//! The seven views read records the journal already produced. Deployment
//! scope is the journal for an admitted current and none when that current
//! was refused or withheld. Avoided inference stays unmeasured.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use serde::Serialize;
use thiserror::Error;

mod sha256;

pub const DASHBOARD_V0: &str = "cyclone.dashboard/v0";
pub const RECORD_SCOPE: &str = "journal";

const VIEWS: [&str; 7] = [
    "Current registry",
    "Cyclone graph",
    "Pressure history",
    "Compilation report",
    "Economic report",
    "Execution trace",
    "Safety report",
];

const FALLBACK_AGENT: &str = "original_agent_workflow";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DashboardError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardSource {
    pub candidates: Vec<CandidateView>,
    pub currents: Vec<CurrentView>,
    pub shadows: Vec<ShadowView>,
    pub graph: GraphView,
    pub admissions: Vec<AdmissionView>,
    pub drifts: Vec<DriftView>,
    pub decisions: Vec<DecisionView>,
    pub actions: Vec<ActionRecord>,
    pub economics: Vec<EconomicsView>,
    pub portfolio_benefit_minor: i64,
    pub portfolio_leaked: bool,
    pub portfolio_refusals: Vec<String>,
    pub compile_refusals: Vec<RefusalView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateView {
    pub signature: Vec<String>,
    pub pressure: String,
    pub promotion: String,
    pub owner: String,
    pub agent_executions: u64,
    pub observations: u64,
    pub economics_minor: i64,
    pub agreement_matching: u64,
    pub agreement_considered: u64,
    pub explanation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentView {
    pub current_id: String,
    pub version: u32,
    pub tenant_id: String,
    pub signature: Vec<String>,
    pub schedule: String,
    pub effects: Vec<String>,
    pub artifact_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowView {
    pub current_id: String,
    pub checked: u64,
    pub passed: u64,
    pub pass: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionView {
    pub current_id: String,
    pub lifecycle: String,
    pub transitions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftView {
    pub kind: String,
    pub current_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionView {
    pub execution_id: String,
    pub current_id: String,
    pub route: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionRecord {
    pub tenant_id: String,
    pub execution_id: String,
    pub sequence: u64,
    pub action_kind: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomicsView {
    pub signature: Vec<String>,
    pub executions: u64,
    pub cost_original: i64,
    pub cost_compiled: i64,
    pub cost_detection: i64,
    pub cost_validation: i64,
    pub cost_maintenance: i64,
    pub cost_risk: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalView {
    pub signature: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DashboardReport {
    pub schema: String,
    pub pass: bool,
    pub record_scope: String,
    pub registry: Vec<RegistryRow>,
    pub graph: GraphView,
    pub pressure_history: Vec<PressurePoint>,
    pub compilation: Vec<CompilationRow>,
    pub economics: EconomicReport,
    pub execution: ExecutionReport,
    pub safety: SafetyReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegistryRow {
    pub signature: Vec<String>,
    pub pressure: String,
    pub version: Option<u32>,
    pub owner: String,
    pub evidence: EvidenceView,
    pub status: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceView {
    pub agent_executions: u64,
    pub observations: u64,
    pub economics_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphView {
    pub graph_id: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub critical_path: Vec<String>,
}

impl GraphView {
    pub fn empty() -> Self {
        Self {
            graph_id: String::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            critical_path: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    pub node_id: String,
    pub signature: Vec<String>,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphEdge {
    pub parent: String,
    pub child: String,
    pub trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PressurePoint {
    pub signature: Vec<String>,
    pub pressure: String,
    pub promotion: String,
    pub agreement_matching: u64,
    pub agreement_considered: u64,
    pub agreement: String,
    pub explanation: Vec<String>,
    pub transitions: Vec<String>,
    pub drift_kinds: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompilationRow {
    pub signature: Vec<String>,
    pub transformations: Vec<String>,
    pub validation: String,
    pub artifact_sha256: Option<String>,
    pub effects: Vec<String>,
    pub refusal: Option<String>,
    pub shadow_checked: u64,
    pub shadow_passed: u64,
    pub shadow_pass: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EconomicReport {
    pub avoided_inference: String,
    pub value_minor: Option<i64>,
    pub break_even: Option<u64>,
    pub benefit_minor: i64,
    pub entries: Vec<EconomicEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EconomicEntry {
    pub signature: Vec<String>,
    pub avoided_inference: String,
    pub value_minor: i64,
    pub overhead_minor: i64,
    pub break_even: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionReport {
    pub actions: Vec<ActionRecord>,
    pub fallbacks: Vec<FallbackEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FallbackEvent {
    pub execution_id: String,
    pub current_id: String,
    pub route: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SafetyReport {
    pub rejected_promotions: Vec<RejectedPromotion>,
    pub policy_violations: Vec<String>,
    pub suspended: Vec<String>,
    pub leaked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RejectedPromotion {
    pub signature: Vec<String>,
    pub reason: String,
}

pub fn artifact_sha256(bytes: &[u8]) -> String {
    let digest = sha256::sha256(bytes);
    let mut out = String::from("sha256:");
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn project(source: &DashboardSource) -> Result<DashboardReport, DashboardError> {
    require_unique(
        "signature",
        source.candidates.iter().map(|row| &row.signature),
    )?;
    require_unique("current", source.currents.iter().map(|row| &row.signature))?;
    require_unique(
        "economics",
        source.economics.iter().map(|row| &row.signature),
    )?;

    let mut registry = Vec::with_capacity(source.candidates.len());
    let mut history = Vec::with_capacity(source.candidates.len());
    let mut compilation = Vec::with_capacity(source.candidates.len());
    let mut rejected = Vec::new();

    for candidate in &source.candidates {
        let current = source
            .currents
            .iter()
            .find(|row| row.signature == candidate.signature);
        let admission = current.and_then(|row| {
            source
                .admissions
                .iter()
                .find(|live| live.current_id == row.current_id)
        });
        let (status, scope, version, owner) = match (current, admission) {
            (Some(row), Some(live)) => (
                live.lifecycle.clone(),
                "journal".to_string(),
                Some(row.version),
                row.tenant_id.clone(),
            ),
            (Some(row), None) => (
                "withheld".to_string(),
                "none".to_string(),
                Some(row.version),
                row.tenant_id.clone(),
            ),
            (None, _) => (
                "refused".to_string(),
                "none".to_string(),
                None,
                candidate.owner.clone(),
            ),
        };
        registry.push(RegistryRow {
            signature: candidate.signature.clone(),
            pressure: candidate.pressure.clone(),
            version,
            owner,
            evidence: EvidenceView {
                agent_executions: candidate.agent_executions,
                observations: candidate.observations,
                economics_minor: candidate.economics_minor,
            },
            status,
            scope,
        });

        let transitions = admission
            .map(|live| live.transitions.clone())
            .unwrap_or_default();
        let mut drift_kinds = Vec::new();
        if let Some(row) = current {
            for drift in &source.drifts {
                if drift.current_id == row.current_id && !drift_kinds.contains(&drift.kind) {
                    drift_kinds.push(drift.kind.clone());
                }
            }
        }
        drift_kinds.sort();
        history.push(PressurePoint {
            signature: candidate.signature.clone(),
            pressure: candidate.pressure.clone(),
            promotion: candidate.promotion.clone(),
            agreement_matching: candidate.agreement_matching,
            agreement_considered: candidate.agreement_considered,
            agreement: agreement_label(
                candidate.agreement_matching,
                candidate.agreement_considered,
            ),
            explanation: candidate.explanation.clone(),
            transitions,
            drift_kinds,
        });

        let refusal = source
            .compile_refusals
            .iter()
            .find(|row| row.signature == candidate.signature);
        let shadow = current.and_then(|row| {
            source
                .shadows
                .iter()
                .find(|check| check.current_id == row.current_id)
        });
        compilation.push(CompilationRow {
            signature: candidate.signature.clone(),
            transformations: current
                .map(|row| vec![row.schedule.clone()])
                .unwrap_or_default(),
            validation: if current.is_some() {
                "passed".to_string()
            } else {
                "refused".to_string()
            },
            artifact_sha256: current.map(|row| row.artifact_sha256.clone()),
            effects: current.map(|row| row.effects.clone()).unwrap_or_default(),
            refusal: refusal.map(|row| row.reason.clone()),
            shadow_checked: shadow.map(|row| row.checked).unwrap_or(0),
            shadow_passed: shadow.map(|row| row.passed).unwrap_or(0),
            shadow_pass: shadow.is_some_and(|row| row.pass),
        });

        if candidate.promotion == "refused" {
            let reason = match refusal {
                Some(row) => row.reason.clone(),
                None if !candidate.explanation.is_empty() => candidate.explanation.join("; "),
                None => "promotion refused".to_string(),
            };
            rejected.push(RejectedPromotion {
                signature: candidate.signature.clone(),
                reason,
            });
        }
    }

    let economics = economics_of(source)?;
    let mut actions = source.actions.clone();
    actions.sort_by(|left, right| {
        (&left.tenant_id, &left.execution_id, left.sequence).cmp(&(
            &right.tenant_id,
            &right.execution_id,
            right.sequence,
        ))
    });
    let fallbacks = source
        .decisions
        .iter()
        .filter(|decision| {
            decision.route == FALLBACK_AGENT
                || decision.outcome == "rolled_back"
                || decision.outcome == "withheld"
        })
        .map(|decision| FallbackEvent {
            execution_id: decision.execution_id.clone(),
            current_id: decision.current_id.clone(),
            route: decision.route.clone(),
            outcome: decision.outcome.clone(),
        })
        .collect();

    let mut suspended: Vec<String> = source
        .admissions
        .iter()
        .filter(|live| live.lifecycle == "suspended")
        .map(|live| live.current_id.clone())
        .collect();
    suspended.sort();
    suspended.dedup();

    let mut policy_violations = Vec::new();
    for drift in &source.drifts {
        if drift.kind == "policy" {
            policy_violations.push(format!("policy {} {}", drift.current_id, drift.detail));
        }
    }
    for refusal in &source.compile_refusals {
        if authority_reason(&refusal.reason) {
            policy_violations.push(format!(
                "{} {}",
                refusal.signature.join(","),
                refusal.reason
            ));
        }
    }
    policy_violations.extend(source.portfolio_refusals.clone());
    policy_violations.sort();
    policy_violations.dedup();

    let scopes_held = registry
        .iter()
        .all(|row| row.scope == "journal" || row.scope == "none");
    Ok(DashboardReport {
        schema: DASHBOARD_V0.to_string(),
        pass: scopes_held && !source.portfolio_leaked,
        record_scope: RECORD_SCOPE.to_string(),
        registry,
        graph: source.graph.clone(),
        pressure_history: history,
        compilation,
        economics,
        execution: ExecutionReport { actions, fallbacks },
        safety: SafetyReport {
            rejected_promotions: rejected,
            policy_violations,
            suspended,
            leaked: source.portfolio_leaked,
        },
    })
}

pub fn render_html(report: &DashboardReport) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<title>Cyclone operator dashboard</title>\n<style>\n");
    out.push_str("body{font-family:sans-serif;margin:2rem;color:#1a1a1a;background:#f7f6f3;}\n");
    out.push_str("main{max-width:64rem;}h2{margin-top:2rem;}\n");
    out.push_str("table{border-collapse:collapse;width:100%;}\n");
    out.push_str(
        "th,td{border:1px solid #ccc;padding:0.4rem 0.6rem;text-align:left;vertical-align:top;}\n",
    );
    out.push_str("th{background:#efece4;}code{font-family:ui-monospace,monospace;}\n");
    out.push_str("</style>\n</head>\n<body>\n<main>\n");
    out.push_str("<h1>Cyclone operator dashboard</h1>\n");
    out.push_str("<p>Deployment scope is the journal. This page is an operator record.</p>\n");
    out.push_str(&format!(
        "<p>Schema <code>{}</code>. Record scope <code>{}</code>. Pass <code>{}</code>.</p>\n",
        escape(&report.schema),
        escape(&report.record_scope),
        report.pass
    ));
    out.push_str(&section_registry(&report.registry));
    out.push_str(&section_graph(&report.graph));
    out.push_str(&section_history(&report.pressure_history));
    out.push_str(&section_compilation(&report.compilation));
    out.push_str(&section_economics(&report.economics));
    out.push_str(&section_execution(&report.execution));
    out.push_str(&section_safety(&report.safety));
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

pub fn read_over_http(html: &str) -> Result<String, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let page = html.to_string();
    let server = std::thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer)?;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            page.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(page.as_bytes())?;
        stream.flush()?;
        Ok(())
    });
    let mut client = TcpStream::connect(address)?;
    client.set_read_timeout(Some(Duration::from_secs(2)))?;
    client.set_write_timeout(Some(Duration::from_secs(2)))?;
    client.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
    let mut received = String::new();
    client.read_to_string(&mut received)?;
    match server.join() {
        Ok(result) => result?,
        Err(_) => return Err(std::io::Error::other("dashboard server stopped")),
    }
    Ok(received)
}

fn require_unique<'a>(
    label: &str,
    rows: impl Iterator<Item = &'a Vec<String>>,
) -> Result<(), DashboardError> {
    let mut seen = BTreeSet::new();
    for signature in rows {
        if !seen.insert(signature) {
            return Err(DashboardError::Message(format!(
                "repeated {label} {}",
                signature.join(",")
            )));
        }
    }
    Ok(())
}

fn agreement_label(matching: u64, considered: u64) -> String {
    if considered == 0 {
        "unmeasured".to_string()
    } else {
        format!("{matching}/{considered}")
    }
}

fn authority_reason(reason: &str) -> bool {
    reason.contains("cross-tenant") || reason.contains("privileged") || reason.contains("authority")
}

fn economics_of(source: &DashboardSource) -> Result<EconomicReport, DashboardError> {
    let mut entries = Vec::new();
    for row in &source.economics {
        if !source
            .candidates
            .iter()
            .any(|candidate| candidate.signature == row.signature)
        {
            continue;
        }
        let per = row
            .cost_original
            .checked_sub(row.cost_compiled)
            .ok_or_else(|| DashboardError::Message("economics value overflow".to_string()))?;
        let overhead = overhead_of(row)?;
        let value = net_value(row.executions, per, overhead)?;
        entries.push(EconomicEntry {
            signature: row.signature.clone(),
            avoided_inference: "unmeasured".to_string(),
            value_minor: value,
            overhead_minor: overhead,
            break_even: break_even_after(per, overhead)?,
        });
    }
    let mut total: Option<i128> = None;
    let mut break_even: Option<u64> = None;
    for entry in &entries {
        let next = i128::from(entry.value_minor);
        total = Some(match total {
            Some(current) => current
                .checked_add(next)
                .ok_or_else(|| DashboardError::Message("economics value overflow".to_string()))?,
            None => next,
        });
        if let Some(count) = entry.break_even {
            break_even = Some(match break_even {
                Some(current) => current.max(count),
                None => count,
            });
        }
    }
    let value_minor = match total {
        Some(sum) => Some(
            i64::try_from(sum)
                .map_err(|_| DashboardError::Message("economics value overflow".to_string()))?,
        ),
        None => None,
    };
    Ok(EconomicReport {
        avoided_inference: "unmeasured".to_string(),
        value_minor,
        break_even,
        benefit_minor: source.portfolio_benefit_minor,
        entries,
    })
}

fn overhead_of(row: &EconomicsView) -> Result<i64, DashboardError> {
    row.cost_detection
        .checked_add(row.cost_validation)
        .and_then(|sum| sum.checked_add(row.cost_maintenance))
        .and_then(|sum| sum.checked_add(row.cost_risk))
        .ok_or_else(|| DashboardError::Message("economics value overflow".to_string()))
}

fn net_value(executions: u64, per: i64, overhead: i64) -> Result<i64, DashboardError> {
    let gross = i128::from(executions)
        .checked_mul(i128::from(per))
        .ok_or_else(|| DashboardError::Message("economics value overflow".to_string()))?;
    let net = gross
        .checked_sub(i128::from(overhead))
        .ok_or_else(|| DashboardError::Message("economics value overflow".to_string()))?;
    i64::try_from(net).map_err(|_| DashboardError::Message("economics value overflow".to_string()))
}

fn break_even_after(per: i64, overhead: i64) -> Result<Option<u64>, DashboardError> {
    if per <= 0 {
        return Ok(None);
    }
    let count = i128::from(overhead).div_euclid(i128::from(per)) + 1;
    let count = u64::try_from(count)
        .map_err(|_| DashboardError::Message("economics value overflow".to_string()))?;
    Ok(Some(count))
}

fn section_registry(rows: &[RegistryRow]) -> String {
    let mut out = format!("<h2>{}</h2>\n", VIEWS[0]);
    if rows.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
        return out;
    }
    out.push_str("<table>\n<tr><th scope=\"col\">Signature</th><th scope=\"col\">Pressure</th><th scope=\"col\">Version</th><th scope=\"col\">Owner</th><th scope=\"col\">Evidence</th><th scope=\"col\">Status</th><th scope=\"col\">Scope</th></tr>\n");
    for row in rows {
        let version = match row.version {
            Some(version) => version.to_string(),
            None => "none".to_string(),
        };
        out.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>agents {}, observations {}, economics_minor {}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&row.signature.join(",")),
            escape(&row.pressure),
            escape(&version),
            escape(&row.owner),
            row.evidence.agent_executions,
            row.evidence.observations,
            row.evidence.economics_minor,
            escape(&row.status),
            escape(&row.scope)
        ));
    }
    out.push_str("</table>\n");
    out
}

fn section_graph(graph: &GraphView) -> String {
    let mut out = format!("<h2>{}</h2>\n", VIEWS[1]);
    if graph.nodes.is_empty() && graph.edges.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
        return out;
    }
    out.push_str(&format!(
        "<p>Graph <code>{}</code>. Critical path <code>{}</code>.</p>\n",
        escape(&graph.graph_id),
        escape(&graph.critical_path.join(","))
    ));
    out.push_str("<table>\n<tr><th scope=\"col\">Parent</th><th scope=\"col\">Child</th><th scope=\"col\">Trigger</th></tr>\n");
    if graph.edges.is_empty() {
        out.push_str("<tr><td colspan=\"3\">No parent-child edge.</td></tr>\n");
    }
    for edge in &graph.edges {
        out.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&edge.parent),
            escape(&edge.child),
            escape(&edge.trigger)
        ));
    }
    out.push_str("</table>\n");
    out
}

fn section_history(points: &[PressurePoint]) -> String {
    let mut out = format!("<h2>{}</h2>\n", VIEWS[2]);
    if points.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
        return out;
    }
    for point in points {
        out.push_str(&format!(
            "<p>{} pressure {} promotion {} agreement {}.</p>\n",
            escape(&point.signature.join(",")),
            escape(&point.pressure),
            escape(&point.promotion),
            escape(&point.agreement)
        ));
        out.push_str(&format!(
            "<p>Transitions {}. Drift causes {}.</p>\n",
            escape(&joined_or_none(&point.transitions)),
            escape(&joined_or_none(&point.drift_kinds))
        ));
        for line in &point.explanation {
            out.push_str(&format!("<p>{}</p>\n", escape(line)));
        }
    }
    out
}

fn section_compilation(rows: &[CompilationRow]) -> String {
    let mut out = format!("<h2>{}</h2>\n", VIEWS[3]);
    if rows.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
        return out;
    }
    for row in rows {
        let hash = row.artifact_sha256.as_deref().unwrap_or("none");
        let refusal = row.refusal.as_deref().unwrap_or("none");
        out.push_str(&format!(
            "<p>{} validation {} schedule {} hash <code>{}</code> effects {} shadow {}/{} pass {}.</p>\n",
            escape(&row.signature.join(",")),
            escape(&row.validation),
            escape(&joined_or_none(&row.transformations)),
            escape(hash),
            escape(&joined_or_none(&row.effects)),
            row.shadow_checked,
            row.shadow_passed,
            row.shadow_pass
        ));
        out.push_str(&format!("<p>Refusal {}.</p>\n", escape(refusal)));
    }
    out
}

fn section_economics(report: &EconomicReport) -> String {
    let mut out = format!("<h2>{}</h2>\n", VIEWS[4]);
    out.push_str(&format!(
        "<p>Avoided inference is {}. Benefit minor {}. Net value {}. Break-even {}.</p>\n",
        escape(&report.avoided_inference),
        report.benefit_minor,
        optional_i64(report.value_minor),
        optional_u64(report.break_even)
    ));
    for entry in &report.entries {
        out.push_str(&format!(
            "<p>{} value {} overhead {} break-even {}.</p>\n",
            escape(&entry.signature.join(",")),
            entry.value_minor,
            entry.overhead_minor,
            optional_u64(entry.break_even)
        ));
    }
    out
}

fn section_execution(report: &ExecutionReport) -> String {
    let mut out = format!("<h2>{}</h2>\n", VIEWS[5]);
    if report.actions.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
    } else {
        out.push_str("<table>\n<tr><th scope=\"col\">Tenant</th><th scope=\"col\">Execution</th><th scope=\"col\">Sequence</th><th scope=\"col\">Action</th><th scope=\"col\">Outcome</th></tr>\n");
        for action in &report.actions {
            out.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                escape(&action.tenant_id),
                escape(&action.execution_id),
                action.sequence,
                escape(&action.action_kind),
                escape(&action.outcome)
            ));
        }
        out.push_str("</table>\n");
    }
    out.push_str("<h3>Fallback events</h3>\n");
    if report.fallbacks.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
        return out;
    }
    for event in &report.fallbacks {
        out.push_str(&format!(
            "<p>{} {} route {} outcome {}.</p>\n",
            escape(&event.execution_id),
            escape(&event.current_id),
            escape(&event.route),
            escape(&event.outcome)
        ));
    }
    out
}

fn section_safety(report: &SafetyReport) -> String {
    let mut out = format!("<h2>{}</h2>\n", VIEWS[6]);
    out.push_str(&format!("<p>Leaked <code>{}</code>.</p>\n", report.leaked));
    out.push_str("<h3>Rejected promotions</h3>\n");
    if report.rejected_promotions.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
    }
    for row in &report.rejected_promotions {
        out.push_str(&format!(
            "<p>{} {}.</p>\n",
            escape(&row.signature.join(",")),
            escape(&row.reason)
        ));
    }
    out.push_str("<h3>Policy violations</h3>\n");
    if report.policy_violations.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
    }
    for row in &report.policy_violations {
        out.push_str(&format!("<p>{}</p>\n", escape(row)));
    }
    out.push_str("<h3>Suspended currents</h3>\n");
    if report.suspended.is_empty() {
        out.push_str("<p>None recorded.</p>\n");
    }
    for row in &report.suspended {
        out.push_str(&format!("<p>{}</p>\n", escape(row)));
    }
    out
}

fn joined_or_none(parts: &[String]) -> String {
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(",")
    }
}

fn optional_i64(value: Option<i64>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "unmeasured".to_string(),
    }
}

fn optional_u64(value: Option<u64>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "none".to_string(),
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_candidate() -> CandidateView {
        CandidateView {
            signature: vec!["read".to_string()],
            pressure: "high".to_string(),
            promotion: "eligible".to_string(),
            owner: "tenant-a".to_string(),
            agent_executions: 30,
            observations: 30,
            economics_minor: 1,
            agreement_matching: 30,
            agreement_considered: 30,
            explanation: vec!["provisional high-pressure gates passed".to_string()],
        }
    }

    fn read_costs(original: i64, compiled: i64, detection: i64) -> EconomicsView {
        EconomicsView {
            signature: vec!["read".to_string()],
            executions: 30,
            cost_original: original,
            cost_compiled: compiled,
            cost_detection: detection,
            cost_validation: 5,
            cost_maintenance: 5,
            cost_risk: 0,
        }
    }

    fn source_with(economics: Vec<EconomicsView>, leaked: bool) -> DashboardSource {
        DashboardSource {
            candidates: vec![read_candidate()],
            currents: Vec::new(),
            shadows: Vec::new(),
            graph: GraphView::empty(),
            admissions: Vec::new(),
            drifts: Vec::new(),
            decisions: Vec::new(),
            actions: Vec::new(),
            economics,
            portfolio_benefit_minor: 0,
            portfolio_leaked: leaked,
            portfolio_refusals: Vec::new(),
            compile_refusals: vec![RefusalView {
                signature: vec!["read".to_string()],
                reason: "promotion refused".to_string(),
            }],
        }
    }

    #[test]
    fn sha256_matches_the_empty_and_abc_vectors() {
        assert_eq!(
            artifact_sha256(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            artifact_sha256(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            artifact_sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "sha256:248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn the_read_formula_breaks_even_at_four() {
        let mut source = source_with(vec![read_costs(10, 1, 20)], false);
        source.portfolio_benefit_minor = 20;
        source.compile_refusals.clear();
        let report = project(&source).unwrap();
        assert_eq!(report.economics.avoided_inference, "unmeasured");
        assert_eq!(report.economics.value_minor, Some(240));
        assert_eq!(report.economics.break_even, Some(4));
        assert_eq!(report.economics.benefit_minor, 20);
        assert_eq!(report.economics.entries[0].overhead_minor, 30);
    }

    #[test]
    fn a_non_positive_saving_has_no_break_even() {
        let flat = project(&source_with(vec![read_costs(1, 1, 20)], false)).unwrap();
        assert_eq!(flat.economics.value_minor, Some(-30));
        assert_eq!(flat.economics.break_even, None);
        assert_eq!(flat.economics.entries[0].break_even, None);

        let loss = project(&source_with(vec![read_costs(1, 5, 20)], false)).unwrap();
        assert!(loss.economics.value_minor.is_some_and(|value| value < 0));
        assert_eq!(loss.economics.break_even, None);
    }

    #[test]
    fn zero_overhead_breaks_even_on_the_first_execution() {
        let entry = EconomicsView {
            signature: vec!["read".to_string()],
            executions: 1,
            cost_original: 2,
            cost_compiled: 1,
            cost_detection: 0,
            cost_validation: 0,
            cost_maintenance: 0,
            cost_risk: 0,
        };
        let report = project(&source_with(vec![entry], false)).unwrap();
        assert_eq!(report.economics.value_minor, Some(1));
        assert_eq!(report.economics.break_even, Some(1));
    }

    #[test]
    fn a_leak_fails_the_projection() {
        let report = project(&source_with(Vec::new(), true)).unwrap();
        assert!(!report.pass);
        assert!(report.safety.leaked);
        assert_eq!(report.record_scope, "journal");
    }

    #[test]
    fn a_compiled_current_absent_from_admission_is_withheld() {
        let mut source = source_with(Vec::new(), false);
        source.compile_refusals.clear();
        source.currents.push(CurrentView {
            current_id: "tenant-a:read".to_string(),
            version: 1,
            tenant_id: "tenant-a".to_string(),
            signature: vec!["read".to_string()],
            schedule: "interpret".to_string(),
            effects: vec!["read_only".to_string()],
            artifact_sha256: artifact_sha256(b"cir"),
        });
        let report = project(&source).unwrap();
        assert!(report.pass);
        assert_eq!(report.registry[0].status, "withheld");
        assert_eq!(report.registry[0].scope, "none");
        assert_eq!(report.compilation[0].transformations, vec!["interpret"]);
        assert_eq!(report.compilation[0].validation, "passed");
    }

    #[test]
    fn a_suspended_lifecycle_stays_on_the_journal() {
        let mut source = source_with(Vec::new(), false);
        source.compile_refusals.clear();
        source.currents.push(CurrentView {
            current_id: "tenant-a:read".to_string(),
            version: 1,
            tenant_id: "tenant-a".to_string(),
            signature: vec!["read".to_string()],
            schedule: "interpret".to_string(),
            effects: vec!["read_only".to_string()],
            artifact_sha256: artifact_sha256(b"cir"),
        });
        source.admissions.push(AdmissionView {
            current_id: "tenant-a:read".to_string(),
            lifecycle: "suspended".to_string(),
            transitions: vec!["shadow->suspended".to_string()],
        });
        source.drifts.push(DriftView {
            kind: "policy".to_string(),
            current_id: "tenant-a:read".to_string(),
            detail: "policy disabled".to_string(),
        });
        let report = project(&source).unwrap();
        assert_eq!(report.registry[0].status, "suspended");
        assert_eq!(report.registry[0].scope, "journal");
        assert_eq!(report.safety.suspended, vec!["tenant-a:read"]);
        assert_eq!(report.pressure_history[0].drift_kinds, vec!["policy"]);
        assert!(report.safety.policy_violations[0].contains("policy"));
    }

    #[test]
    fn html_escapes_and_names_the_seven_views() {
        let mut source = source_with(Vec::new(), false);
        source.candidates[0].owner = "a&b<c>\"d'e".to_string();
        source.candidates[0].promotion = "refused".to_string();
        let report = project(&source).unwrap();
        let html = render_html(&report);
        for title in VIEWS {
            assert!(html.contains(&format!("<h2>{title}</h2>")), "{title}");
        }
        assert!(html.contains("a&amp;b&lt;c&gt;&quot;d&#39;e"));
        assert!(!html.contains("a&b<c>"));
        assert!(html.contains("Deployment scope is the journal."));
        let served = read_over_http(&html).unwrap();
        for title in VIEWS {
            assert!(served.contains(&format!("<h2>{title}</h2>")), "{title}");
        }
        let value = serde_json::to_value(&report).unwrap();
        assert_secret_free(&value);
    }

    fn assert_secret_free(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    assert_ne!(key, "prompt");
                    assert_ne!(key, "secret");
                    assert_secret_free(child);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    assert_secret_free(item);
                }
            }
            _ => {}
        }
    }
}
