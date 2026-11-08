//! Tenant-isolated registry and shared reuse economics.
//!
//! An owning artifact keeps that tenant's provenance. A neutral artifact
//! carries the behaviour key and nothing that names the source tenant.
//! A consumer adopts it only under a fresh grant whose operations stay
//! inside the artifact. Reuse benefit is avoided detection cost. A
//! non-positive value is refused.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use cyclone_ir::{validate, CurrentIr, IoRow};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const REGISTRY_V0: &str = "cyclone.registry/v0";
pub const PORTFOLIO_V0: &str = "cyclone.portfolio/v0";
pub const ECONOMICS_V0: &str = "cyclone.economics/v0";
pub const GRANT_V0: &str = "cyclone.grant/v0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("{0}")]
    Refused(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomicsEntry {
    pub signature: Vec<String>,
    pub executions: u64,
    pub cost_original: i64,
    pub cost_compiled: i64,
    pub cost_detection: i64,
    pub cost_validation: i64,
    pub cost_maintenance: i64,
    pub cost_risk: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Economics {
    pub schema: String,
    pub entries: Vec<EconomicsEntry>,
}

impl Economics {
    pub fn empty() -> Self {
        Self {
            schema: ECONOMICS_V0.to_string(),
            entries: Vec::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        let economics: Self = serde_json::from_str(text)
            .map_err(|error| RegistryError::Refused(format!("economics: {error}")))?;
        economics.check()?;
        Ok(economics)
    }

    pub fn check(&self) -> Result<(), RegistryError> {
        if self.schema != ECONOMICS_V0 {
            return Err(refused("schema must be cyclone.economics/v0"));
        }
        for entry in &self.entries {
            if entry.signature.is_empty() || entry.executions == 0 {
                return Err(refused("economics entry needs a signature and executions"));
            }
            for cost in [
                entry.cost_original,
                entry.cost_compiled,
                entry.cost_detection,
                entry.cost_validation,
                entry.cost_maintenance,
                entry.cost_risk,
            ] {
                if cost < 0 {
                    return Err(refused("economics costs must be non-negative"));
                }
            }
            if value_of(entry).is_none() {
                return Err(refused("economics value overflow"));
            }
        }
        Ok(())
    }

    fn lookup(&self, signature: &[String]) -> Option<&EconomicsEntry> {
        self.entries
            .iter()
            .find(|entry| entry.signature == signature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub schema: String,
    pub source_tenant: String,
    pub consumer_tenant: String,
    pub signature: Vec<String>,
    pub operations: Vec<String>,
    pub principal: String,
}

impl Grant {
    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        let grant: Self = serde_json::from_str(text)
            .map_err(|error| RegistryError::Refused(format!("grant: {error}")))?;
        grant.check()?;
        Ok(grant)
    }

    pub fn check(&self) -> Result<(), RegistryError> {
        if self.schema != GRANT_V0 {
            return Err(refused("schema must be cyclone.grant/v0"));
        }
        if self.source_tenant.is_empty()
            || self.consumer_tenant.is_empty()
            || self.principal.is_empty()
            || self.signature.is_empty()
            || self.operations.is_empty()
        {
            return Err(refused(
                "grant needs tenant, principal, signature, and operations",
            ));
        }
        if self.source_tenant == self.consumer_tenant {
            return Err(refused("grant repeats the owning tenant"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub schema: String,
    pub artifact_id: String,
    pub tenant_id: String,
    pub neutral: bool,
    pub signature: Vec<String>,
    pub environment: String,
    pub table: Vec<IoRow>,
    pub side_effects: Vec<String>,
    pub idempotency: String,
    pub pressure: String,
    pub operations: Vec<String>,
    pub agent_ids: Vec<String>,
    pub observation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pattern {
    pub tenant_id: String,
    pub signature: Vec<String>,
    pub agents: Vec<String>,
    pub artifact_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReuseDecision {
    pub consumer_tenant: String,
    pub artifact_id: String,
    pub adopted: bool,
    pub reason: String,
    pub benefit_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioReport {
    pub schema: String,
    pub pass: bool,
    pub patterns: Vec<Pattern>,
    pub artifacts: Vec<Artifact>,
    pub reuses: Vec<ReuseDecision>,
    pub refusals: Vec<String>,
    pub benefit_minor: i64,
    pub leaked: bool,
}

pub fn portfolio(
    currents: &[CurrentIr],
    agents: &BTreeMap<(String, Vec<String>), BTreeSet<String>>,
    economics: &Economics,
    consumers: &[String],
    grant: Option<&Grant>,
) -> Result<PortfolioReport, RegistryError> {
    economics.check()?;
    if let Some(grant) = grant {
        grant.check()?;
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for current in currents {
        if !seen.insert(current.current_id.as_str()) {
            return Err(refused("repeated current"));
        }
    }

    let mut owned: Vec<Artifact> = Vec::new();
    let mut refusals: Vec<String> = Vec::new();
    for current in currents {
        match publish(current, agents) {
            Ok(artifact) => owned.push(artifact),
            Err(error) => refusals.push(error.to_string()),
        }
    }

    let mut patterns = Vec::new();
    let mut reuses = Vec::new();
    for artifact in &owned {
        let key = (artifact.tenant_id.clone(), artifact.signature.clone());
        let names = agents.get(&key).cloned().unwrap_or_default();
        if names.len() < 2 {
            continue;
        }
        let agents_list: Vec<String> = names.into_iter().collect();
        patterns.push(Pattern {
            tenant_id: artifact.tenant_id.clone(),
            signature: artifact.signature.clone(),
            agents: agents_list.clone(),
            artifact_id: artifact.artifact_id.clone(),
        });
        match shared_benefit(economics, &artifact.signature, agents_list.len()) {
            Benefit::Unmeasured => {
                refusals.push(format!(
                    "economics unmeasured {}",
                    artifact.signature.join(",")
                ));
            }
            Benefit::Refused => {
                refusals.push(format!(
                    "economics refused {}",
                    artifact.signature.join(",")
                ));
            }
            Benefit::Value(benefit) => reuses.push(ReuseDecision {
                consumer_tenant: artifact.tenant_id.clone(),
                artifact_id: artifact.artifact_id.clone(),
                adopted: true,
                reason: format!("shared by {} agents", agents_list.len()),
                benefit_minor: benefit,
            }),
        }
    }

    let mut published = owned.clone();
    if let Some(grant) = grant {
        match federate(&owned, economics, grant) {
            Ok((neutral, consumer, benefit)) => {
                published.push(neutral);
                published.push(consumer.clone());
                reuses.push(ReuseDecision {
                    consumer_tenant: consumer.tenant_id.clone(),
                    artifact_id: consumer.artifact_id.clone(),
                    adopted: true,
                    reason: "fresh authorisation".to_string(),
                    benefit_minor: benefit,
                });
            }
            Err(error) => refusals.push(error.to_string()),
        }
    }
    for consumer in consumers {
        let covered = grant.is_some_and(|grant| &grant.consumer_tenant == consumer);
        let already = owned.iter().any(|artifact| &artifact.tenant_id == consumer);
        if !covered && !already {
            refusals.push(format!("authorisation missing {consumer}"));
        }
    }

    published.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    patterns.sort_by(|left, right| {
        left.tenant_id
            .cmp(&right.tenant_id)
            .then(left.signature.cmp(&right.signature))
    });
    reuses.sort_by(|left, right| {
        left.consumer_tenant
            .cmp(&right.consumer_tenant)
            .then(left.reason.cmp(&right.reason))
    });
    refusals.sort();
    let leaked = published.iter().any(|artifact| leaks(artifact, &owned));
    let adopted_ok = reuses
        .iter()
        .filter(|reuse| reuse.adopted)
        .all(|reuse| reuse.benefit_minor > 0);
    let benefit_minor = reuses
        .iter()
        .filter(|reuse| reuse.adopted)
        .map(|reuse| reuse.benefit_minor)
        .sum();
    Ok(PortfolioReport {
        schema: PORTFOLIO_V0.to_string(),
        pass: !leaked && adopted_ok,
        patterns,
        artifacts: published,
        reuses,
        refusals,
        benefit_minor,
        leaked,
    })
}

fn publish(
    current: &CurrentIr,
    agents: &BTreeMap<(String, Vec<String>), BTreeSet<String>>,
) -> Result<Artifact, RegistryError> {
    if privileged(current) {
        return Err(refused("privileged effect"));
    }
    validate(current).map_err(|error| refused(&format!("validation: {error}")))?;
    let key = (current.tenant_id.clone(), current.signature.clone());
    let agent_ids: Vec<String> = agents
        .get(&key)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    Ok(Artifact {
        schema: REGISTRY_V0.to_string(),
        artifact_id: current.current_id.clone(),
        tenant_id: current.tenant_id.clone(),
        neutral: false,
        signature: current.signature.clone(),
        environment: current.environment.digest.clone(),
        table: current.table.clone(),
        side_effects: current.side_effects.clone(),
        idempotency: current.idempotency.clone(),
        pressure: current.pressure.clone(),
        operations: current.signature.clone(),
        agent_ids,
        observation_ids: current.evidence.observation_ids.clone(),
    })
}

fn federate(
    owned: &[Artifact],
    economics: &Economics,
    grant: &Grant,
) -> Result<(Artifact, Artifact, i64), RegistryError> {
    let source = owned
        .iter()
        .find(|artifact| {
            artifact.tenant_id == grant.source_tenant && artifact.signature == grant.signature
        })
        .ok_or_else(|| refused("source artifact missing"))?;
    if owned.iter().any(|artifact| {
        artifact.tenant_id == grant.consumer_tenant && artifact.signature == grant.signature
    }) {
        return Err(refused("consumer already holds the signature"));
    }
    if grant
        .operations
        .iter()
        .any(|operation| !source.operations.contains(operation))
        || grant.signature != source.signature
    {
        return Err(refused("authority escalation"));
    }
    let benefit = match shared_benefit(economics, &source.signature, 2) {
        Benefit::Value(benefit) => benefit,
        Benefit::Unmeasured => return Err(refused("economics unmeasured")),
        Benefit::Refused => return Err(refused("economics refused")),
    };
    let neutral_id = format!(
        "neutral:{}:{}",
        source.signature.join(","),
        source.environment
    );
    if id_names_tenant(&neutral_id, &source.tenant_id) {
        return Err(refused("neutral id names the source tenant"));
    }
    let neutral = Artifact {
        schema: REGISTRY_V0.to_string(),
        artifact_id: neutral_id,
        tenant_id: "neutral".to_string(),
        neutral: true,
        signature: source.signature.clone(),
        environment: source.environment.clone(),
        table: source.table.clone(),
        side_effects: source.side_effects.clone(),
        idempotency: source.idempotency.clone(),
        pressure: source.pressure.clone(),
        operations: source.operations.clone(),
        agent_ids: Vec::new(),
        observation_ids: Vec::new(),
    };
    let consumer = Artifact {
        schema: REGISTRY_V0.to_string(),
        artifact_id: format!("{}:{}", grant.consumer_tenant, grant.signature.join(",")),
        tenant_id: grant.consumer_tenant.clone(),
        neutral: false,
        signature: source.signature.clone(),
        environment: source.environment.clone(),
        table: source.table.clone(),
        side_effects: source.side_effects.clone(),
        idempotency: source.idempotency.clone(),
        pressure: source.pressure.clone(),
        operations: grant.operations.clone(),
        agent_ids: Vec::new(),
        observation_ids: Vec::new(),
    };
    Ok((neutral, consumer, benefit))
}

enum Benefit {
    Value(i64),
    Unmeasured,
    Refused,
}

fn shared_benefit(economics: &Economics, signature: &[String], adopters: usize) -> Benefit {
    if adopters < 2 {
        return Benefit::Refused;
    }
    let Some(entry) = economics.lookup(signature) else {
        return Benefit::Unmeasured;
    };
    match value_of(entry) {
        Some(value) if value > 0 => {
            let extra = i64::try_from(adopters - 1).unwrap_or(i64::MAX);
            match entry.cost_detection.checked_mul(extra) {
                Some(benefit) if benefit > 0 => Benefit::Value(benefit),
                _ => Benefit::Refused,
            }
        }
        Some(_) => Benefit::Refused,
        None => Benefit::Refused,
    }
}

fn value_of(entry: &EconomicsEntry) -> Option<i64> {
    let per = entry.cost_original.checked_sub(entry.cost_compiled)?;
    let gross = i128::from(entry.executions).checked_mul(i128::from(per))?;
    let overhead = i128::from(entry.cost_detection)
        + i128::from(entry.cost_validation)
        + i128::from(entry.cost_maintenance)
        + i128::from(entry.cost_risk);
    let net = gross.checked_sub(overhead)?;
    i64::try_from(net).ok()
}

fn privileged(current: &CurrentIr) -> bool {
    let outside = |effect: &str| effect != "pure" && effect != "read_only";
    current.steps.iter().any(|step| outside(&step.effect))
        || current.side_effects.iter().any(|effect| outside(effect))
}

fn leaks(record: &Artifact, owned: &[Artifact]) -> bool {
    if record.neutral {
        if !record.agent_ids.is_empty() || !record.observation_ids.is_empty() {
            return true;
        }
        return owned.iter().any(|owner| {
            id_names_tenant(&record.artifact_id, &owner.tenant_id)
                || record.tenant_id == owner.tenant_id
        });
    }
    for owner in owned {
        if owner.tenant_id == record.tenant_id && owner.artifact_id == record.artifact_id {
            continue;
        }
        if record
            .observation_ids
            .iter()
            .any(|id| owner.observation_ids.contains(id))
        {
            return true;
        }
        if record
            .agent_ids
            .iter()
            .any(|id| owner.agent_ids.contains(id))
        {
            return true;
        }
        if id_names_tenant(&record.artifact_id, &owner.tenant_id) {
            return true;
        }
    }
    false
}

fn id_names_tenant(artifact_id: &str, tenant: &str) -> bool {
    artifact_id.split(':').any(|part| part == tenant)
}

fn refused(message: &str) -> RegistryError {
    RegistryError::Refused(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyclone_ir::{
        Environment, Evidence, IoRow, Postcondition, Precondition, Retry, Step, CIR_V0,
        FALLBACK_AGENT, SCHEDULE_INTERPRET,
    };

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn current(tenant: &str, observation: &str) -> CurrentIr {
        let environment = digest('3');
        let signature = vec!["read".to_string()];
        CurrentIr {
            schema: CIR_V0.to_string(),
            current_id: format!("{tenant}:read"),
            version: 1,
            tenant_id: tenant.to_string(),
            signature: signature.clone(),
            pressure: "high".to_string(),
            steps: vec![Step {
                action_kind: "read".to_string(),
                effect: "read_only".to_string(),
            }],
            table: vec![IoRow {
                input: vec![digest('1')],
                output: vec![digest('2')],
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
                observation_ids: vec![observation.to_string()],
            },
        }
    }

    fn economics() -> Economics {
        Economics::parse(include_str!(
            "../../../traces/portfolio-read.economics.json"
        ))
        .unwrap()
    }

    fn agents(tenant: &str, names: &[&str]) -> BTreeMap<(String, Vec<String>), BTreeSet<String>> {
        let mut map = BTreeMap::new();
        map.insert(
            (tenant.to_string(), vec!["read".to_string()]),
            names.iter().map(|name| (*name).to_string()).collect(),
        );
        map
    }

    fn grant() -> Grant {
        Grant::parse(include_str!(
            "../../../traces/portfolio-consumer.grant.json"
        ))
        .unwrap()
    }

    #[test]
    fn two_agents_share_one_compilation() {
        let read = current("tenant-a", "alpha-00");
        let report = portfolio(
            &[read],
            &agents("tenant-a", &["beta", "alpha"]),
            &economics(),
            &[],
            None,
        )
        .unwrap();
        assert!(report.pass);
        assert!(!report.leaked);
        assert_eq!(report.schema, "cyclone.portfolio/v0");
        assert_eq!(report.patterns.len(), 1);
        assert_eq!(
            report.patterns[0].agents,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(report.benefit_minor, 20);
        assert!(report.reuses[0].adopted);
        assert!(report
            .artifacts
            .iter()
            .all(|artifact| artifact.tenant_id == "tenant-a"));
        let value = serde_json::to_value(&report.reuses[0]).unwrap();
        assert!(value.get("prompt").is_none());
        assert!(value.get("secret").is_none());
    }

    #[test]
    fn a_foreign_consumer_without_a_grant_is_refused() {
        let read = current("tenant-a", "alpha-00");
        let report = portfolio(
            &[read],
            &agents("tenant-a", &["alpha", "beta"]),
            &economics(),
            &["tenant-b".to_string()],
            None,
        )
        .unwrap();
        assert!(report.pass);
        assert!(report
            .refusals
            .iter()
            .any(|refusal| refusal.contains("authorisation missing")));
        assert!(report
            .artifacts
            .iter()
            .all(|artifact| artifact.tenant_id != "tenant-b"));
        assert_eq!(report.benefit_minor, 20);
    }

    #[test]
    fn a_grant_adopts_a_neutral_artifact_without_the_source_tenant() {
        let read = current("tenant-a", "alpha-00");
        let authorisation = grant();
        let report = portfolio(
            &[read],
            &agents("tenant-a", &["alpha", "beta"]),
            &economics(),
            &[authorisation.consumer_tenant.clone()],
            Some(&authorisation),
        )
        .unwrap();
        assert!(report.pass, "{:?}", report.refusals);
        assert!(!report.leaked);
        assert_eq!(report.benefit_minor, 40);
        let consumer = report
            .artifacts
            .iter()
            .find(|artifact| artifact.tenant_id == "tenant-b")
            .unwrap();
        assert!(consumer.observation_ids.is_empty());
        assert!(consumer.agent_ids.is_empty());
        assert_eq!(consumer.artifact_id, "tenant-b:read");
        let consumer_json = serde_json::to_string(consumer).unwrap();
        assert!(!consumer_json.contains("tenant-a"));
        assert!(!consumer_json.contains("alpha-00"));
        let neutral = report
            .artifacts
            .iter()
            .find(|artifact| artifact.neutral)
            .unwrap();
        assert!(neutral.observation_ids.is_empty());
        assert!(neutral.artifact_id.starts_with("neutral:"));
        let neutral_json = serde_json::to_string(neutral).unwrap();
        assert!(!neutral_json.contains("tenant-a"));
        assert!(!neutral_json.contains("alpha"));
    }

    #[test]
    fn an_extra_operation_is_an_authority_escalation() {
        let read = current("tenant-a", "alpha-00");
        let mut authorisation = grant();
        authorisation.operations.push("write_file".to_string());
        let report = portfolio(
            &[read],
            &agents("tenant-a", &["alpha", "beta"]),
            &economics(),
            &[],
            Some(&authorisation),
        )
        .unwrap();
        assert!(report.pass);
        assert!(report
            .refusals
            .iter()
            .any(|refusal| refusal.contains("authority escalation")));
        assert!(report
            .artifacts
            .iter()
            .all(|artifact| artifact.tenant_id != "tenant-b"));
        assert_eq!(report.benefit_minor, 20);
    }

    #[test]
    fn a_non_positive_value_refuses_reuse() {
        let read = current("tenant-a", "alpha-00");
        let mut sheet = economics();
        sheet.entries[0].cost_original = 1;
        sheet.entries[0].cost_compiled = 1;
        let report = portfolio(
            &[read],
            &agents("tenant-a", &["alpha", "beta"]),
            &sheet,
            &[],
            None,
        )
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.benefit_minor, 0);
        assert!(report.reuses.is_empty());
        assert!(report
            .refusals
            .iter()
            .any(|refusal| refusal.contains("economics refused")));
    }

    #[test]
    fn a_privileged_current_is_not_registered() {
        let mut read = current("tenant-a", "alpha-00");
        read.steps[0].effect = "filesystem_mutation".to_string();
        read.side_effects = vec!["filesystem_mutation".to_string()];
        let report = portfolio(
            &[read],
            &agents("tenant-a", &["alpha", "beta"]),
            &economics(),
            &[],
            None,
        )
        .unwrap();
        assert!(report.pass);
        assert!(report.artifacts.is_empty());
        assert!(report
            .refusals
            .iter()
            .any(|refusal| refusal.contains("privileged effect")));
        assert_eq!(report.benefit_minor, 0);
    }

    #[test]
    fn one_agent_does_not_count_as_a_shared_pattern() {
        let read = current("tenant-a", "alpha-00");
        let report = portfolio(
            &[read],
            &agents("tenant-a", &["alpha"]),
            &economics(),
            &[],
            None,
        )
        .unwrap();
        assert!(report.pass);
        assert!(report.patterns.is_empty());
        assert_eq!(report.benefit_minor, 0);
        assert_eq!(report.artifacts.len(), 1);
    }

    #[test]
    fn copied_provenance_fails_the_isolation_check() {
        let owner = current("tenant-a", "alpha-00");
        let mut consumer = publish(&owner, &agents("tenant-a", &["alpha"])).unwrap();
        consumer.tenant_id = "tenant-b".to_string();
        consumer.artifact_id = "tenant-b:read".to_string();
        let owned = vec![publish(&owner, &agents("tenant-a", &["alpha"])).unwrap()];
        assert!(leaks(&consumer, &owned));
        consumer.observation_ids.clear();
        consumer.agent_ids.clear();
        assert!(!leaks(&consumer, &owned));
    }
}
