//! Compose compiled currents into a typed execution graph and schedule it.
//!
//! A cyclone is nodes, causal edges, timing constraints, and guards. The
//! scheduler runs each current as a digest lookup. A child starts after every
//! parent trigger is satisfied. Independent currents share a wave. The wave
//! is the concurrency contract, and the log serializes that wave in node-id
//! order so a second run matches the first. Pure lookups with the same
//! environment and input are evaluated once. An unbounded cycle is refused.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cyclone_ir::{validate, CurrentIr};
use cyclone_observe::{ActionObservation, ActionOutcome, EffectClass, Trace};
use cyclone_verify::{interpret, InterpretResult};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const GRAPH_V0: &str = "cyclone.graph/v0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ComposeError {
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Invalid(#[from] cyclone_ir::IrError),
    #[error(transparent)]
    Verify(#[from] cyclone_verify::VerifyError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycloneGraph {
    pub schema: String,
    pub graph_id: String,
    pub tenant_id: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub waves: Vec<Vec<String>>,
    pub critical_path: Vec<String>,
    pub shared: Vec<SharedComputation>,
    pub guards: Guards,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub node_id: String,
    pub current_id: String,
    pub signature: Vec<String>,
    pub pressure: String,
    pub side_effects: Vec<String>,
    pub idempotency: String,
    pub environment: String,
    pub repeat: u32,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub parent: String,
    pub child: String,
    pub trigger: String,
    pub event: Option<String>,
    pub milestone: Option<u32>,
    pub input_bindings: Vec<InputBinding>,
    pub deadline_ms: Option<u64>,
    pub cancellation: String,
    pub failure: String,
    pub inherit_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputBinding {
    pub parent_output: usize,
    pub child_input: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedComputation {
    pub key: String,
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guards {
    pub child_waits_for_trigger: bool,
    pub cancellation: String,
    pub failure: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRun {
    pub order: Vec<String>,
    pub cancelled: Vec<String>,
    pub blocked: Vec<String>,
    pub evaluations: u64,
    pub reuses: u64,
    pub effects: Vec<String>,
    pub pass: bool,
    pub explanation: Vec<String>,
}

#[derive(Clone)]
struct StepDraft {
    current_id: String,
    signature: Vec<String>,
    input: Vec<String>,
    output: Vec<String>,
    boundary_before: Option<String>,
}

pub fn compose(currents: &[CurrentIr], traces: &[Trace]) -> Result<CycloneGraph, ComposeError> {
    if currents.is_empty() {
        return Ok(empty_graph());
    }
    let mut by_signature: BTreeMap<(String, Vec<String>), &CurrentIr> = BTreeMap::new();
    let mut tenant: Option<&str> = None;
    for current in currents {
        validate(current)?;
        if current
            .steps
            .iter()
            .any(|step| step.effect != "pure" && step.effect != "read_only")
        {
            return Err(refused("privileged effect"));
        }
        if current.retry.max_attempts > 3 {
            return Err(refused("unbounded repeat"));
        }
        match tenant {
            None => tenant = Some(current.tenant_id.as_str()),
            Some(seen) if seen != current.tenant_id => return Err(refused("cross-tenant")),
            Some(_) => {}
        }
        let key = (current.tenant_id.clone(), current.signature.clone());
        if by_signature.insert(key, current).is_some() {
            return Err(refused("repeated current"));
        }
    }
    let tenant = tenant.unwrap_or("").to_string();

    let mut observed: BTreeSet<String> = BTreeSet::new();
    let mut edge_drafts: BTreeMap<(String, String), Edge> = BTreeMap::new();
    for trace in traces {
        let steps = steps_in_trace(trace, &by_signature)?;
        let mut seen_in_trace: BTreeSet<String> = BTreeSet::new();
        for step in &steps {
            if !seen_in_trace.insert(step.current_id.clone()) {
                return Err(refused("repeated current"));
            }
            observed.insert(step.current_id.clone());
        }
        for pair in steps.windows(2) {
            let parent = by_signature
                .get(&(trace.tenant_id.clone(), pair[0].signature.clone()))
                .copied()
                .ok_or_else(|| refused("uncompiled segment"))?;
            let child = by_signature
                .get(&(trace.tenant_id.clone(), pair[1].signature.clone()))
                .copied()
                .ok_or_else(|| refused("uncompiled segment"))?;
            let edge = edge_between(parent, child, &pair[0], &pair[1]);
            let key = (edge.parent.clone(), edge.child.clone());
            if let Some(existing) = edge_drafts.get(&key) {
                if existing.trigger != edge.trigger
                    || existing.input_bindings != edge.input_bindings
                    || existing.event != edge.event
                    || existing.milestone != edge.milestone
                {
                    return Err(refused("trigger conflict"));
                }
            } else {
                edge_drafts.insert(key, edge);
            }
        }
    }

    for current in currents {
        if !observed.contains(&current.current_id) {
            return Err(refused("unobserved current"));
        }
    }

    let mut nodes = Vec::with_capacity(currents.len());
    for current in currents {
        let mut parents: Vec<String> = edge_drafts
            .values()
            .filter(|edge| edge.child == current.current_id)
            .map(|edge| edge.parent.clone())
            .collect();
        parents.sort();
        parents.dedup();
        nodes.push(Node {
            node_id: current.current_id.clone(),
            current_id: current.current_id.clone(),
            signature: current.signature.clone(),
            pressure: current.pressure.clone(),
            side_effects: current.side_effects.clone(),
            idempotency: current.idempotency.clone(),
            environment: current.environment.digest.clone(),
            repeat: current.retry.max_attempts,
            parents,
        });
    }
    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));

    let mut edges: Vec<Edge> = edge_drafts.into_values().collect();
    edges.sort_by(|left, right| (&left.parent, &left.child).cmp(&(&right.parent, &right.child)));
    for edge in &edges {
        let parent = currents
            .iter()
            .find(|current| current.current_id == edge.parent)
            .ok_or_else(|| refused("uncompiled segment"))?;
        let child = currents
            .iter()
            .find(|current| current.current_id == edge.child)
            .ok_or_else(|| refused("uncompiled segment"))?;
        if !bindings_hold(parent, child, &edge.input_bindings) {
            return Err(refused("unvalidated binding"));
        }
    }

    let node_ids: Vec<String> = nodes.iter().map(|node| node.node_id.clone()).collect();
    let waves = waves_of(&node_ids, &edges)?;
    let critical_path = critical_path_of(&node_ids, &edges);
    let shared = shared_of(currents);
    let mut signatures: Vec<String> = nodes.iter().map(|node| node.signature.join(",")).collect();
    signatures.sort();
    signatures.dedup();
    Ok(CycloneGraph {
        schema: GRAPH_V0.to_string(),
        graph_id: format!("{tenant}:{}", signatures.join("+")),
        tenant_id: tenant,
        nodes,
        edges,
        waves,
        critical_path,
        shared,
        guards: Guards {
            child_waits_for_trigger: true,
            cancellation: "propagate".to_string(),
            failure: "fail_graph".to_string(),
        },
    })
}

pub fn run_graph(
    graph: &CycloneGraph,
    currents: &[CurrentIr],
) -> Result<ScheduleRun, ComposeError> {
    let mut by_id: BTreeMap<&str, &CurrentIr> = BTreeMap::new();
    for current in currents {
        by_id.insert(current.current_id.as_str(), current);
    }
    let mut status: BTreeMap<&str, Status> = BTreeMap::new();
    for node in &graph.nodes {
        status.insert(node.node_id.as_str(), Status::Pending);
    }
    let mut order = Vec::new();
    let mut cancelled = Vec::new();
    let mut blocked = Vec::new();
    let mut effects = Vec::new();
    let mut explanation = Vec::new();
    let mut evaluations = 0_u64;
    let mut reuses = 0_u64;
    let mut cache: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for wave in &graph.waves {
        for node_id in wave {
            let node = graph
                .nodes
                .iter()
                .find(|node| node.node_id == *node_id)
                .ok_or_else(|| refused("schedule incomplete"))?;
            let incoming: Vec<&Edge> = graph
                .edges
                .iter()
                .filter(|edge| edge.child == node.node_id)
                .collect();
            let parents_ready = incoming
                .iter()
                .all(|edge| matches!(status.get(edge.parent.as_str()), Some(Status::Completed)));
            if !parents_ready {
                let propagate = incoming.iter().any(|edge| {
                    edge.cancellation == "propagate"
                        && matches!(
                            status.get(edge.parent.as_str()),
                            Some(Status::Failed | Status::Cancelled)
                        )
                });
                if propagate {
                    status.insert(node.node_id.as_str(), Status::Cancelled);
                    cancelled.push(node.node_id.clone());
                } else {
                    status.insert(node.node_id.as_str(), Status::Blocked);
                    blocked.push(node.node_id.clone());
                }
                continue;
            }
            if incoming.iter().any(|edge| edge.deadline_ms == Some(0)) {
                status.insert(node.node_id.as_str(), Status::Cancelled);
                cancelled.push(node.node_id.clone());
                continue;
            }
            let current = by_id
                .get(node.current_id.as_str())
                .copied()
                .ok_or_else(|| refused("unobserved current"))?;
            if !incoming.iter().all(|edge| {
                let parent = by_id.get(edge.parent.as_str()).copied();
                parent
                    .map(|parent| bindings_hold(parent, current, &edge.input_bindings))
                    .unwrap_or(false)
            }) {
                status.insert(node.node_id.as_str(), Status::Failed);
                explanation.push(format!("fail: binding {}", node.node_id));
                continue;
            }
            if node.repeat == 0 || node.repeat > 3 {
                status.insert(node.node_id.as_str(), Status::Failed);
                explanation.push(format!("fail: unbounded repeat {}", node.node_id));
                continue;
            }
            let mut failed = false;
            for row in &current.table {
                let key = cache_key(
                    &node.idempotency,
                    &current.environment.digest,
                    &row.input,
                    &row.output,
                );
                let output = if let Some(cached) = cache.get(&key) {
                    reuses += 1;
                    cached.clone()
                } else {
                    let produced =
                        match interpret(current, &row.input, &current.environment.digest)? {
                            InterpretResult::Value { output } => output,
                            InterpretResult::Fallback { reason } => {
                                explanation.push(format!("fail: {reason} {}", node.node_id));
                                failed = true;
                                Vec::new()
                            }
                        };
                    if !failed {
                        evaluations += 1;
                        cache.insert(key, produced.clone());
                    }
                    produced
                };
                if failed {
                    break;
                }
                if output != row.output {
                    explanation.push(format!("fail: output {}", node.node_id));
                    failed = true;
                    break;
                }
                for _ in 1..node.repeat {
                    reuses += 1;
                    let again = cache.get(&cache_key(
                        &node.idempotency,
                        &current.environment.digest,
                        &row.input,
                        &row.output,
                    ));
                    if again != Some(&output) {
                        explanation.push(format!("fail: repeat {}", node.node_id));
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                status.insert(node.node_id.as_str(), Status::Failed);
                continue;
            }
            status.insert(node.node_id.as_str(), Status::Completed);
            order.push(node.node_id.clone());
            effects.extend(node.side_effects.iter().cloned());
        }
    }

    for edge in &graph.edges {
        if let (Some(parent_at), Some(child_at)) = (
            order.iter().position(|id| id == &edge.parent),
            order.iter().position(|id| id == &edge.child),
        ) {
            if parent_at > child_at {
                explanation.push(format!("fail: order {} before {}", edge.child, edge.parent));
            }
        } else if order.iter().any(|id| id == &edge.child)
            && !order.iter().any(|id| id == &edge.parent)
        {
            explanation.push(format!(
                "fail: child {} before parent {}",
                edge.child, edge.parent
            ));
        }
    }

    cancelled.sort();
    blocked.sort();
    let pass = explanation.is_empty();
    Ok(ScheduleRun {
        order,
        cancelled,
        blocked,
        evaluations,
        reuses,
        effects,
        pass,
        explanation,
    })
}

#[derive(Clone, Copy)]
enum Status {
    Pending,
    Completed,
    Cancelled,
    Blocked,
    Failed,
}

fn steps_in_trace(
    trace: &Trace,
    currents: &BTreeMap<(String, Vec<String>), &CurrentIr>,
) -> Result<Vec<StepDraft>, ComposeError> {
    let mut steps = Vec::new();
    let mut open: Vec<&ActionObservation> = Vec::new();
    let mut boundary: Option<String> = None;
    for observation in &trace.observations {
        if is_boundary(observation) {
            flush_segment(trace, currents, &open, boundary.clone(), &mut steps)?;
            open.clear();
            boundary = Some(observation.action_kind.clone());
            continue;
        }
        open.push(observation);
    }
    flush_segment(trace, currents, &open, boundary, &mut steps)?;
    Ok(steps)
}

fn flush_segment(
    trace: &Trace,
    currents: &BTreeMap<(String, Vec<String>), &CurrentIr>,
    open: &[&ActionObservation],
    boundary_before: Option<String>,
    steps: &mut Vec<StepDraft>,
) -> Result<(), ComposeError> {
    if open.is_empty() {
        return Ok(());
    }
    let signature: Vec<String> = open.iter().map(|row| row.action_kind.clone()).collect();
    let current = currents
        .get(&(trace.tenant_id.clone(), signature.clone()))
        .copied()
        .ok_or_else(|| refused("uncompiled segment"))?;
    let mut input = Vec::with_capacity(open.len());
    let mut output = Vec::with_capacity(open.len());
    let mut environment: Option<String> = None;
    for row in open {
        let Some(in_digest) = row.input_digest.clone() else {
            return Err(refused("digest missing"));
        };
        let Some(out_digest) = row.output_digest.clone() else {
            return Err(refused("digest missing"));
        };
        let Some(env_digest) = row.environment_digest.clone() else {
            return Err(refused("digest missing"));
        };
        if let Some(seen) = &environment {
            if seen != &env_digest {
                return Err(refused("environment mismatch"));
            }
        } else {
            environment = Some(env_digest.clone());
        }
        if env_digest != current.environment.digest {
            return Err(refused("environment mismatch"));
        }
        input.push(in_digest);
        output.push(out_digest);
    }
    if !current
        .table
        .iter()
        .any(|row| row.input == input && row.output == output)
    {
        return Err(refused("unvalidated output"));
    }
    steps.push(StepDraft {
        current_id: current.current_id.clone(),
        signature,
        input,
        output,
        boundary_before,
    });
    Ok(())
}

fn edge_between(
    parent: &CurrentIr,
    child: &CurrentIr,
    parent_step: &StepDraft,
    child_step: &StepDraft,
) -> Edge {
    let mut bindings = Vec::new();
    for (child_index, input) in child_step.input.iter().enumerate() {
        if let Some(parent_index) = parent_step.output.iter().position(|output| output == input) {
            bindings.push(InputBinding {
                parent_output: parent_index,
                child_input: child_index,
            });
        }
    }
    let (trigger, milestone) = if !bindings.is_empty() {
        let early = parent.steps.len() > 1
            && bindings
                .iter()
                .any(|binding| binding.parent_output + 1 < parent.steps.len());
        if early {
            let milestone = bindings
                .iter()
                .map(|binding| binding.parent_output as u32)
                .min()
                .unwrap_or(0);
            ("phase", Some(milestone))
        } else {
            ("data", None)
        }
    } else if matches!(
        child_step.boundary_before.as_deref(),
        Some("task.completed" | "task.running" | "finish" | "message.applied")
    ) {
        ("sequential", None)
    } else {
        ("event", None)
    };
    let inherit_context =
        child.pressure == "low" && parent.environment.digest == child.environment.digest;
    Edge {
        parent: parent.current_id.clone(),
        child: child.current_id.clone(),
        trigger: trigger.to_string(),
        event: child_step.boundary_before.clone(),
        milestone,
        input_bindings: bindings,
        deadline_ms: None,
        cancellation: "propagate".to_string(),
        failure: "fail_graph".to_string(),
        inherit_context,
    }
}

fn bindings_hold(parent: &CurrentIr, child: &CurrentIr, bindings: &[InputBinding]) -> bool {
    bindings.iter().all(|binding| {
        child.table.iter().all(|child_row| {
            let Some(expected) = child_row.input.get(binding.child_input) else {
                return false;
            };
            parent
                .table
                .iter()
                .any(|parent_row| parent_row.output.get(binding.parent_output) == Some(expected))
        })
    })
}

fn waves_of(nodes: &[String], edges: &[Edge]) -> Result<Vec<Vec<String>>, ComposeError> {
    let mut indegree: BTreeMap<&str, usize> = nodes.iter().map(|node| (node.as_str(), 0)).collect();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in edges {
        if edge.parent == edge.child || !indegree.contains_key(edge.parent.as_str()) {
            return Err(refused("unbounded cycle"));
        }
        *indegree
            .get_mut(edge.child.as_str())
            .ok_or_else(|| refused("unbounded cycle"))? += 1;
        outgoing
            .entry(edge.parent.as_str())
            .or_default()
            .push(edge.child.as_str());
    }
    let mut ready: VecDeque<&str> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(node, _)| *node)
        .collect();
    let mut waves = Vec::new();
    let mut seen = 0_usize;
    while !ready.is_empty() {
        let mut wave: Vec<&str> = ready.drain(..).collect();
        wave.sort_unstable();
        seen += wave.len();
        let mut next = Vec::new();
        for node in &wave {
            if let Some(children) = outgoing.get(node) {
                for child in children {
                    let degree = indegree
                        .get_mut(child)
                        .ok_or_else(|| refused("unbounded cycle"))?;
                    *degree -= 1;
                    if *degree == 0 {
                        next.push(*child);
                    }
                }
            }
        }
        waves.push(wave.into_iter().map(str::to_string).collect());
        ready.extend(next);
    }
    if seen != nodes.len() {
        return Err(refused("unbounded cycle"));
    }
    Ok(waves)
}

fn critical_path_of(nodes: &[String], edges: &[Edge]) -> Vec<String> {
    let mut predecessors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in edges {
        predecessors
            .entry(edge.child.as_str())
            .or_default()
            .push(edge.parent.as_str());
    }
    let waves = waves_of(nodes, edges).unwrap_or_default();
    let mut best: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for wave in &waves {
        for node in wave {
            let mut path = vec![node.clone()];
            if let Some(parents) = predecessors.get(node.as_str()) {
                for parent in parents {
                    if let Some(parent_path) = best.get(parent) {
                        let mut candidate = parent_path.clone();
                        candidate.push(node.clone());
                        if candidate.len() > path.len()
                            || (candidate.len() == path.len() && candidate < path)
                        {
                            path = candidate;
                        }
                    }
                }
            }
            best.insert(node.as_str(), path);
        }
    }
    best.into_values()
        .max_by(|left, right| left.len().cmp(&right.len()).then(right.cmp(left)))
        .unwrap_or_default()
}

fn shared_of(currents: &[CurrentIr]) -> Vec<SharedComputation> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for current in currents {
        if current.table.len() != 1 {
            continue;
        }
        if current.idempotency != "pure" && current.idempotency != "read_only" {
            continue;
        }
        let row = &current.table[0];
        let key = cache_key(
            &current.idempotency,
            &current.environment.digest,
            &row.input,
            &row.output,
        );
        groups
            .entry(key)
            .or_default()
            .push(current.current_id.clone());
    }
    let mut shared = Vec::new();
    for (key, mut nodes) in groups {
        nodes.sort();
        nodes.dedup();
        if nodes.len() > 1 {
            shared.push(SharedComputation { key, nodes });
        }
    }
    shared.sort_by(|left, right| left.key.cmp(&right.key));
    shared
}

fn cache_key(idempotency: &str, environment: &str, input: &[String], output: &[String]) -> String {
    format!(
        "{idempotency}|{environment}|{}|{}",
        input.join(","),
        output.join(",")
    )
}

fn empty_graph() -> CycloneGraph {
    CycloneGraph {
        schema: GRAPH_V0.to_string(),
        graph_id: "empty".to_string(),
        tenant_id: String::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        waves: Vec::new(),
        critical_path: Vec::new(),
        shared: Vec::new(),
        guards: Guards {
            child_waits_for_trigger: true,
            cancellation: "propagate".to_string(),
            failure: "fail_graph".to_string(),
        },
    }
}

fn is_boundary(observation: &ActionObservation) -> bool {
    observation.generative
        || observation.effect_class == EffectClass::Generative
        || observation.effect_class == EffectClass::Control
        || observation.outcome != ActionOutcome::Success
}

fn refused(reason: &str) -> ComposeError {
    ComposeError::Refused(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyclone_ir::{
        Environment, Evidence, IoRow, Postcondition, Precondition, Retry, Step, CIR_V0,
        FALLBACK_AGENT, SCHEDULE_INTERPRET,
    };
    use cyclone_observe::{ActionObservation, ObservationSource};

    const ENV: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
    const IN_A: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const OUT_A: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    const OUT_B: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
    const IN_C: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
    const OUT_C: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
    const OTHER_ENV: &str =
        "sha256:7777777777777777777777777777777777777777777777777777777777777777";

    fn current(
        signature: &[&str],
        effect: &str,
        pressure: &str,
        rows: &[(&[&str], &[&str])],
        environment: &str,
    ) -> CurrentIr {
        let idempotency = if effect == "pure" {
            "pure"
        } else {
            "read_only"
        };
        let attempts = if effect == "pure" { 3 } else { 1 };
        let executions = if pressure == "high" { 30 } else { 15 };
        let signature: Vec<String> = signature.iter().map(|part| (*part).to_string()).collect();
        let steps = signature
            .iter()
            .map(|action| Step {
                action_kind: action.clone(),
                effect: effect.to_string(),
            })
            .collect();
        let mut table: Vec<IoRow> = rows
            .iter()
            .map(|(input, output)| IoRow {
                input: input.iter().map(|part| (*part).to_string()).collect(),
                output: output.iter().map(|part| (*part).to_string()).collect(),
            })
            .collect();
        table.sort_by(|left, right| left.input.cmp(&right.input));
        CurrentIr {
            schema: CIR_V0.to_string(),
            current_id: format!("tenant-a:{}", signature.join(",")),
            version: 1,
            tenant_id: "tenant-a".to_string(),
            signature,
            pressure: pressure.to_string(),
            steps,
            table,
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
            fallback: FALLBACK_AGENT.to_string(),
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
        execution: &str,
        sequence: u64,
        action: &str,
        input: Option<&str>,
        output: Option<&str>,
        environment: Option<&str>,
    ) -> ActionObservation {
        let mut row = ActionObservation::new(execution, sequence, action);
        row.tenant_id = "tenant-a".to_string();
        row.input_digest = input.map(str::to_string);
        row.output_digest = output.map(str::to_string);
        row.environment_digest = environment.map(str::to_string);
        row
    }

    fn trace(execution: &str, observations: Vec<ActionObservation>) -> Trace {
        Trace {
            trace_id: execution.to_string(),
            tenant_id: "tenant-a".to_string(),
            execution_id: execution.to_string(),
            source: ObservationSource::Agent,
            observations,
        }
    }

    #[test]
    fn a_chain_preserves_order_and_effects() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let read = current(&["read"], "read_only", "high", &[(&[OUT_A], &[OUT_B])], ENV);
        let normalise = current(&["normalise"], "pure", "high", &[(&[IN_C], &[OUT_C])], ENV);
        let chain = trace(
            "exec-chain",
            vec![
                observation("exec-chain", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-chain", 2, "model.response", None, None, None),
                observation("exec-chain", 3, "read", Some(OUT_A), Some(OUT_B), Some(ENV)),
                observation("exec-chain", 4, "task.completed", None, None, None),
                observation(
                    "exec-chain",
                    5,
                    "normalise",
                    Some(IN_C),
                    Some(OUT_C),
                    Some(ENV),
                ),
            ],
        );
        let graph = compose(&[pure.clone(), read.clone(), normalise.clone()], &[chain]).unwrap();
        assert_eq!(graph.edges.len(), 2);
        assert_eq!(graph.edges[0].trigger, "data");
        assert_eq!(graph.edges[0].event.as_deref(), Some("model.response"));
        assert_eq!(graph.edges[1].trigger, "sequential");
        assert_eq!(graph.waves.len(), 3);
        assert_eq!(graph.critical_path.len(), 3);
        let currents = [pure, read, normalise];
        let run = run_graph(&graph, &currents).unwrap();
        assert!(run.pass, "{:?}", run.explanation);
        assert_eq!(
            run.order,
            vec![
                "tenant-a:pure".to_string(),
                "tenant-a:read".to_string(),
                "tenant-a:normalise".to_string()
            ]
        );
        assert_eq!(run.effects, vec!["pure", "read_only", "pure"]);
        assert_eq!(run.evaluations, 3);
        let again = run_graph(&graph, &currents).unwrap();
        assert_eq!(run, again);
    }

    #[test]
    fn identical_pure_lookups_are_shared() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let normalise = current(&["normalise"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let journal = trace(
            "exec-share",
            vec![
                observation("exec-share", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-share", 2, "task.completed", None, None, None),
                observation(
                    "exec-share",
                    3,
                    "normalise",
                    Some(IN_A),
                    Some(OUT_A),
                    Some(ENV),
                ),
            ],
        );
        let currents = [pure, normalise];
        let graph = compose(&currents, &[journal]).unwrap();
        assert_eq!(graph.shared.len(), 1);
        assert_eq!(graph.edges[0].trigger, "sequential");
        let run = run_graph(&graph, &currents).unwrap();
        assert!(run.pass, "{:?}", run.explanation);
        assert_eq!(run.evaluations, 1);
        assert!(run.reuses >= 1);
        assert_eq!(
            run.order,
            vec![
                "tenant-a:pure".to_string(),
                "tenant-a:normalise".to_string()
            ]
        );
    }

    #[test]
    fn independent_currents_share_one_wave() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let normalise = current(&["normalise"], "pure", "high", &[(&[IN_C], &[OUT_C])], ENV);
        let left = trace(
            "exec-pure",
            vec![observation(
                "exec-pure",
                1,
                "pure",
                Some(IN_A),
                Some(OUT_A),
                Some(ENV),
            )],
        );
        let right = trace(
            "exec-norm",
            vec![observation(
                "exec-norm",
                1,
                "normalise",
                Some(IN_C),
                Some(OUT_C),
                Some(ENV),
            )],
        );
        let currents = [pure, normalise];
        let graph = compose(&currents, &[left, right]).unwrap();
        assert!(graph.edges.is_empty());
        assert_eq!(graph.waves.len(), 1);
        assert_eq!(graph.critical_path.len(), 1);
        let run = run_graph(&graph, &currents).unwrap();
        assert!(run.pass, "{:?}", run.explanation);
        assert_eq!(
            run.order,
            vec![
                "tenant-a:normalise".to_string(),
                "tenant-a:pure".to_string()
            ]
        );
    }

    #[test]
    fn reversed_order_is_an_unbounded_cycle() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let read = current(&["read"], "read_only", "high", &[(&[IN_C], &[OUT_C])], ENV);
        let forward = trace(
            "exec-forward",
            vec![
                observation(
                    "exec-forward",
                    1,
                    "pure",
                    Some(IN_A),
                    Some(OUT_A),
                    Some(ENV),
                ),
                observation("exec-forward", 2, "task.completed", None, None, None),
                observation(
                    "exec-forward",
                    3,
                    "read",
                    Some(IN_C),
                    Some(OUT_C),
                    Some(ENV),
                ),
            ],
        );
        let backward = trace(
            "exec-backward",
            vec![
                observation(
                    "exec-backward",
                    1,
                    "read",
                    Some(IN_C),
                    Some(OUT_C),
                    Some(ENV),
                ),
                observation("exec-backward", 2, "task.completed", None, None, None),
                observation(
                    "exec-backward",
                    3,
                    "pure",
                    Some(IN_A),
                    Some(OUT_A),
                    Some(ENV),
                ),
            ],
        );
        let error = compose(&[pure, read], &[forward, backward]).unwrap_err();
        assert!(matches!(error, ComposeError::Refused(reason) if reason == "unbounded cycle"));
    }

    #[test]
    fn a_zero_deadline_cancels_the_child_and_its_descendant() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let read = current(&["read"], "read_only", "high", &[(&[IN_C], &[OUT_B])], ENV);
        let normalise = current(&["normalise"], "pure", "high", &[(&[OUT_C], &[OUT_A])], ENV);
        let chain = trace(
            "exec-deadline",
            vec![
                observation(
                    "exec-deadline",
                    1,
                    "pure",
                    Some(IN_A),
                    Some(OUT_A),
                    Some(ENV),
                ),
                observation("exec-deadline", 2, "task.completed", None, None, None),
                observation(
                    "exec-deadline",
                    3,
                    "read",
                    Some(IN_C),
                    Some(OUT_B),
                    Some(ENV),
                ),
                observation("exec-deadline", 4, "task.completed", None, None, None),
                observation(
                    "exec-deadline",
                    5,
                    "normalise",
                    Some(OUT_C),
                    Some(OUT_A),
                    Some(ENV),
                ),
            ],
        );
        let currents = [pure, read, normalise];
        let mut graph = compose(&currents, &[chain]).unwrap();
        for edge in &mut graph.edges {
            if edge.child == "tenant-a:read" {
                edge.deadline_ms = Some(0);
            }
        }
        let run = run_graph(&graph, &currents).unwrap();
        assert!(run.pass, "{:?}", run.explanation);
        assert_eq!(run.order, vec!["tenant-a:pure".to_string()]);
        assert_eq!(
            run.cancelled,
            vec![
                "tenant-a:normalise".to_string(),
                "tenant-a:read".to_string()
            ]
        );
    }

    #[test]
    fn fan_in_waits_for_both_parents() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let normalise = current(&["normalise"], "pure", "high", &[(&[IN_C], &[OUT_C])], ENV);
        let read = current(&["read"], "read_only", "high", &[(&[OUT_B], &[OUT_B])], ENV);
        let left = trace(
            "exec-left",
            vec![
                observation("exec-left", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-left", 2, "task.completed", None, None, None),
                observation("exec-left", 3, "read", Some(OUT_B), Some(OUT_B), Some(ENV)),
            ],
        );
        let right = trace(
            "exec-right",
            vec![
                observation(
                    "exec-right",
                    1,
                    "normalise",
                    Some(IN_C),
                    Some(OUT_C),
                    Some(ENV),
                ),
                observation("exec-right", 2, "task.completed", None, None, None),
                observation("exec-right", 3, "read", Some(OUT_B), Some(OUT_B), Some(ENV)),
            ],
        );
        let currents = [pure, normalise, read];
        let graph = compose(&currents, &[left, right]).unwrap();
        assert_eq!(graph.waves.len(), 2);
        assert_eq!(graph.critical_path.len(), 2);
        let first = run_graph(&graph, &currents).unwrap();
        let second = run_graph(&graph, &currents).unwrap();
        assert!(first.pass && second.pass);
        assert_eq!(first, second);
        assert_eq!(first.order.last().unwrap(), "tenant-a:read");
        let read_node = graph
            .nodes
            .iter()
            .find(|node| node.node_id == "tenant-a:read")
            .unwrap();
        assert_eq!(
            read_node.parents,
            vec![
                "tenant-a:normalise".to_string(),
                "tenant-a:pure".to_string()
            ]
        );
    }

    #[test]
    fn an_early_output_binding_is_phase_synchronous() {
        let parent = current(
            &["pure", "pure"],
            "pure",
            "high",
            &[(&[IN_A, IN_C], &[OUT_A, OUT_C])],
            ENV,
        );
        let child = current(&["read"], "read_only", "high", &[(&[OUT_A], &[OUT_B])], ENV);
        let journal = trace(
            "exec-phase",
            vec![
                observation("exec-phase", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-phase", 2, "pure", Some(IN_C), Some(OUT_C), Some(ENV)),
                observation("exec-phase", 3, "task.completed", None, None, None),
                observation("exec-phase", 4, "read", Some(OUT_A), Some(OUT_B), Some(ENV)),
            ],
        );
        let currents = [parent, child];
        let graph = compose(&currents, &[journal]).unwrap();
        assert_eq!(graph.edges[0].trigger, "phase");
        assert_eq!(graph.edges[0].milestone, Some(0));
        let run = run_graph(&graph, &currents).unwrap();
        assert!(run.pass, "{:?}", run.explanation);
        assert_eq!(run.order[0], "tenant-a:pure,pure");
        assert_eq!(run.order[1], "tenant-a:read");
    }

    #[test]
    fn a_model_event_is_event_synchronous() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let normalise = current(&["normalise"], "pure", "high", &[(&[IN_C], &[OUT_C])], ENV);
        let journal = trace(
            "exec-event",
            vec![
                observation("exec-event", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-event", 2, "model.response", None, None, None),
                observation(
                    "exec-event",
                    3,
                    "normalise",
                    Some(IN_C),
                    Some(OUT_C),
                    Some(ENV),
                ),
            ],
        );
        let graph = compose(&[pure, normalise], &[journal]).unwrap();
        assert_eq!(graph.edges[0].trigger, "event");
        assert_eq!(graph.edges[0].event.as_deref(), Some("model.response"));
    }

    #[test]
    fn low_pressure_inherits_a_matching_environment() {
        let parent = current(&["pure"], "pure", "low", &[(&[IN_A], &[OUT_A])], ENV);
        let child = current(&["read"], "read_only", "low", &[(&[IN_C], &[OUT_B])], ENV);
        let journal = trace(
            "exec-low",
            vec![
                observation("exec-low", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-low", 2, "task.completed", None, None, None),
                observation("exec-low", 3, "read", Some(IN_C), Some(OUT_B), Some(ENV)),
            ],
        );
        let graph = compose(&[parent, child], &[journal]).unwrap();
        assert!(graph.edges[0].inherit_context);
    }

    #[test]
    fn a_different_environment_stays_on_the_child() {
        let parent = current(&["pure"], "pure", "low", &[(&[IN_A], &[OUT_A])], ENV);
        let child = current(
            &["read"],
            "read_only",
            "low",
            &[(&[IN_C], &[OUT_B])],
            OTHER_ENV,
        );
        let mut child_obs = observation(
            "exec-env",
            3,
            "read",
            Some(IN_C),
            Some(OUT_B),
            Some(OTHER_ENV),
        );
        child_obs.environment_digest = Some(OTHER_ENV.to_string());
        let journal = trace(
            "exec-env",
            vec![
                observation("exec-env", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-env", 2, "task.completed", None, None, None),
                child_obs,
            ],
        );
        let graph = compose(&[parent, child], &[journal]).unwrap();
        assert!(!graph.edges[0].inherit_context);
    }

    #[test]
    fn an_output_outside_the_table_is_unvalidated() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let journal = trace(
            "exec-bad",
            vec![observation(
                "exec-bad",
                1,
                "pure",
                Some(IN_A),
                Some(OUT_B),
                Some(ENV),
            )],
        );
        let error = compose(&[pure], &[journal]).unwrap_err();
        assert!(matches!(error, ComposeError::Refused(reason) if reason == "unvalidated output"));
    }

    #[test]
    fn disagreeing_triggers_are_refused() {
        let pure = current(&["pure"], "pure", "high", &[(&[IN_A], &[OUT_A])], ENV);
        let read = current(
            &["read"],
            "read_only",
            "low",
            &[(&[OUT_A], &[OUT_B]), (&[IN_C], &[OUT_C])],
            ENV,
        );
        let data = trace(
            "exec-data",
            vec![
                observation("exec-data", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-data", 2, "task.completed", None, None, None),
                observation("exec-data", 3, "read", Some(OUT_A), Some(OUT_B), Some(ENV)),
            ],
        );
        let event = trace(
            "exec-event",
            vec![
                observation("exec-event", 1, "pure", Some(IN_A), Some(OUT_A), Some(ENV)),
                observation("exec-event", 2, "task.completed", None, None, None),
                observation("exec-event", 3, "read", Some(IN_C), Some(OUT_C), Some(ENV)),
            ],
        );
        let error = compose(&[pure, read], &[data, event]).unwrap_err();
        assert!(matches!(error, ComposeError::Refused(reason) if reason == "trigger conflict"));
    }
}
