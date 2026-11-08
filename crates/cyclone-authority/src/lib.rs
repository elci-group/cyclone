//! Authority contexts for compiled currents.
//!
//! An owner context names the observed principals, the environment digest,
//! and the signature operations. A consumer context names the grant
//! principal. Historical observation adds no further permission.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use serde::Serialize;
use thiserror::Error;

pub const AUTHORITY_V0: &str = "cyclone.authority/v0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthorityError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritySource {
    pub currents: Vec<CurrentClaim>,
    pub refusals: Vec<RefusalClaim>,
    pub grant: Option<GrantClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentClaim {
    pub current_id: String,
    pub tenant_id: String,
    pub signature: Vec<String>,
    pub effects: Vec<String>,
    pub environment: String,
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalClaim {
    pub signature: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantClaim {
    pub source_tenant: String,
    pub consumer_tenant: String,
    pub principal: String,
    pub signature: Vec<String>,
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorityReport {
    pub schema: String,
    pub pass: bool,
    pub elevated: bool,
    pub historical_permission: bool,
    pub contexts: Vec<Context>,
    pub unauthorised: Vec<Unauthorised>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Context {
    pub current_id: String,
    pub binding: String,
    pub tenant_id: String,
    pub principals: Vec<String>,
    pub resources: Vec<String>,
    pub operations: Vec<String>,
    pub elevated: bool,
    pub historical_permission: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unauthorised {
    pub signature: Vec<String>,
    pub reason: String,
    pub elevated: bool,
    pub historical_permission: bool,
}

pub fn declare(source: &AuthoritySource) -> Result<AuthorityReport, AuthorityError> {
    let mut seen = BTreeSet::new();
    let mut contexts = Vec::new();
    let mut unauthorised = Vec::new();
    let mut principal_gap = false;
    for refusal in &source.refusals {
        unauthorised.push(unauthorised_row(
            refusal.signature.clone(),
            refusal.reason.clone(),
        ));
    }
    for current in &source.currents {
        if !seen.insert(current.current_id.as_str()) {
            return Err(AuthorityError::Message(format!(
                "repeated current {}",
                current.current_id
            )));
        }
        if current.tenant_id.is_empty() {
            return Err(AuthorityError::Message(format!(
                "tenant missing {}",
                current.current_id
            )));
        }
        if current.signature.is_empty() {
            return Err(AuthorityError::Message(format!(
                "operations missing {}",
                current.current_id
            )));
        }
        if current.environment.is_empty() {
            return Err(AuthorityError::Message(format!(
                "resource missing {}",
                current.current_id
            )));
        }
        if !supported(&current.effects) {
            unauthorised.push(unauthorised_row(
                current.signature.clone(),
                "privileged effect".to_string(),
            ));
            continue;
        }
        let principals = principals_of(&current.agents);
        if principals.is_empty() {
            principal_gap = true;
            unauthorised.push(unauthorised_row(
                current.signature.clone(),
                "principal missing".to_string(),
            ));
            continue;
        }
        contexts.push(Context {
            current_id: current.current_id.clone(),
            binding: "owner".to_string(),
            tenant_id: current.tenant_id.clone(),
            principals,
            resources: vec![current.environment.clone()],
            operations: current.signature.clone(),
            elevated: false,
            historical_permission: false,
        });
    }
    if let Some(grant) = &source.grant {
        authorise_grant(grant, &source.currents, &mut contexts, &mut unauthorised)?;
    }
    let pass = !principal_gap
        && contexts.iter().all(|row| context_holds(row, source))
        && unauthorised
            .iter()
            .all(|row| !row.elevated && !row.historical_permission);
    Ok(AuthorityReport {
        schema: AUTHORITY_V0.to_string(),
        pass,
        elevated: false,
        historical_permission: false,
        contexts,
        unauthorised,
    })
}

fn authorise_grant(
    grant: &GrantClaim,
    currents: &[CurrentClaim],
    contexts: &mut Vec<Context>,
    unauthorised: &mut Vec<Unauthorised>,
) -> Result<(), AuthorityError> {
    if grant.principal.is_empty() {
        return Err(AuthorityError::Message(
            "grant principal missing".to_string(),
        ));
    }
    if grant.source_tenant.is_empty() || grant.consumer_tenant.is_empty() {
        return Err(AuthorityError::Message("grant tenant missing".to_string()));
    }
    if grant.source_tenant == grant.consumer_tenant {
        return Err(AuthorityError::Message(
            "grant repeats the owning tenant".to_string(),
        ));
    }
    if grant.signature.is_empty() || grant.operations.is_empty() {
        return Err(AuthorityError::Message(
            "grant operations missing".to_string(),
        ));
    }
    let Some(source) = currents.iter().find(|current| {
        current.tenant_id == grant.source_tenant && current.signature == grant.signature
    }) else {
        unauthorised.push(unauthorised_row(
            grant.signature.clone(),
            "source artifact missing".to_string(),
        ));
        return Ok(());
    };
    if !supported(&source.effects) || principals_of(&source.agents).is_empty() {
        return Ok(());
    }
    if grant
        .operations
        .iter()
        .any(|operation| !source.signature.contains(operation))
    {
        unauthorised.push(unauthorised_row(
            grant.signature.clone(),
            "authority escalation".to_string(),
        ));
        return Ok(());
    }
    let source_principals = principals_of(&source.agents);
    if source_principals
        .iter()
        .any(|principal| principal == &grant.principal)
        || grant.principal == grant.source_tenant
    {
        unauthorised.push(unauthorised_row(
            grant.signature.clone(),
            "principal repeats a source agent".to_string(),
        ));
        return Ok(());
    }
    let current_id = format!("{}:{}", grant.consumer_tenant, grant.signature.join(","));
    if names_tenant(&current_id, &grant.source_tenant) {
        unauthorised.push(unauthorised_row(
            grant.signature.clone(),
            "consumer names the source tenant".to_string(),
        ));
        return Ok(());
    }
    let operations: Vec<String> = source
        .signature
        .iter()
        .filter(|operation| grant.operations.iter().any(|granted| granted == *operation))
        .cloned()
        .collect();
    contexts.push(Context {
        current_id,
        binding: "consumer".to_string(),
        tenant_id: grant.consumer_tenant.clone(),
        principals: vec![grant.principal.clone()],
        resources: vec![source.environment.clone()],
        operations,
        elevated: false,
        historical_permission: false,
    });
    Ok(())
}

fn context_holds(row: &Context, source: &AuthoritySource) -> bool {
    if row.elevated
        || row.historical_permission
        || row.principals.is_empty()
        || row.resources.len() != 1
    {
        return false;
    }
    if row.principals.iter().any(|principal| principal.is_empty()) || row.operations.is_empty() {
        return false;
    }
    match row.binding.as_str() {
        "owner" => source.currents.iter().any(|current| {
            current.current_id == row.current_id
                && current.tenant_id == row.tenant_id
                && current.signature == row.operations
                && row.resources == [current.environment.clone()]
                && row.principals == principals_of(&current.agents)
        }),
        "consumer" => source.grant.as_ref().is_some_and(|grant| {
            let Some(origin) = source.currents.iter().find(|current| {
                current.tenant_id == grant.source_tenant && current.signature == grant.signature
            }) else {
                return false;
            };
            row.tenant_id == grant.consumer_tenant
                && row.principals == [grant.principal.clone()]
                && row
                    .operations
                    .iter()
                    .all(|operation| origin.signature.contains(operation))
                && !row.principals.iter().any(|principal| {
                    principals_of(&origin.agents)
                        .iter()
                        .any(|agent| agent == principal)
                })
                && !names_tenant(&row.current_id, &grant.source_tenant)
                && row.resources == [origin.environment.clone()]
        }),
        _ => false,
    }
}

fn principals_of(agents: &[String]) -> Vec<String> {
    let mut names: Vec<String> = agents
        .iter()
        .filter(|agent| !agent.is_empty())
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    names
}

fn supported(effects: &[String]) -> bool {
    !effects.is_empty()
        && effects
            .iter()
            .all(|effect| effect == "pure" || effect == "read_only")
}

fn names_tenant(value: &str, tenant: &str) -> bool {
    value.split(':').any(|part| part == tenant)
}

fn unauthorised_row(signature: Vec<String>, reason: String) -> Unauthorised {
    Unauthorised {
        signature,
        reason,
        elevated: false,
        historical_permission: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENV: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";

    fn claim(agents: &[&str]) -> CurrentClaim {
        CurrentClaim {
            current_id: "tenant-a:read".to_string(),
            tenant_id: "tenant-a".to_string(),
            signature: vec!["read".to_string()],
            effects: vec!["read_only".to_string()],
            environment: ENV.to_string(),
            agents: agents.iter().map(|agent| (*agent).to_string()).collect(),
        }
    }

    fn grant() -> GrantClaim {
        GrantClaim {
            source_tenant: "tenant-a".to_string(),
            consumer_tenant: "tenant-b".to_string(),
            principal: "portfolio-clerk".to_string(),
            signature: vec!["read".to_string()],
            operations: vec!["read".to_string()],
        }
    }

    #[test]
    fn an_owner_context_names_the_observed_principal() {
        let report = declare(&AuthoritySource {
            currents: vec![claim(&["preflight"])],
            refusals: Vec::new(),
            grant: None,
        })
        .unwrap();
        assert!(report.pass);
        assert!(!report.elevated);
        assert!(!report.historical_permission);
        assert_eq!(report.contexts[0].binding, "owner");
        assert_eq!(report.contexts[0].tenant_id, "tenant-a");
        assert_eq!(report.contexts[0].principals, vec!["preflight"]);
        assert_eq!(report.contexts[0].resources, vec![ENV]);
        assert_eq!(report.contexts[0].operations, vec!["read"]);
        assert!(!report.contexts[0].elevated);
        assert!(!report.contexts[0].historical_permission);
    }

    #[test]
    fn a_filesystem_effect_stays_unauthorised() {
        let mut current = claim(&["preflight"]);
        current.effects = vec!["filesystem_mutation".to_string()];
        let report = declare(&AuthoritySource {
            currents: vec![current],
            refusals: vec![RefusalClaim {
                signature: vec!["write_file".to_string()],
                reason: "effect class is not pure or read_only".to_string(),
            }],
            grant: None,
        })
        .unwrap();
        assert!(report.pass);
        assert!(report.contexts.is_empty());
        assert_eq!(report.unauthorised[1].reason, "privileged effect");
        assert!(!report.unauthorised[1].historical_permission);
        let value = serde_json::to_value(&report).unwrap();
        assert!(value.get("prompt").is_none());
        assert!(value.get("secret").is_none());
    }

    #[test]
    fn a_missing_principal_fails_the_context() {
        let report = declare(&AuthoritySource {
            currents: vec![claim(&[])],
            refusals: Vec::new(),
            grant: None,
        })
        .unwrap();
        assert!(!report.pass);
        assert!(report.contexts.is_empty());
        assert_eq!(report.unauthorised[0].reason, "principal missing");
        assert!(!report.historical_permission);
    }

    #[test]
    fn a_grant_names_the_consumer_principal() {
        let mut current = claim(&["beta", "alpha"]);
        current.agents = vec!["beta".to_string(), "alpha".to_string()];
        let report = declare(&AuthoritySource {
            currents: vec![current],
            refusals: Vec::new(),
            grant: Some(grant()),
        })
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.contexts[0].principals, vec!["alpha", "beta"]);
        let consumer = &report.contexts[1];
        assert_eq!(consumer.binding, "consumer");
        assert_eq!(consumer.current_id, "tenant-b:read");
        assert_eq!(consumer.tenant_id, "tenant-b");
        assert_eq!(consumer.principals, vec!["portfolio-clerk"]);
        assert_eq!(consumer.operations, vec!["read"]);
        assert_eq!(consumer.resources, vec![ENV]);
        let encoded = serde_json::to_string(consumer).unwrap();
        assert!(!encoded.contains("tenant-a"));
        assert!(!encoded.contains("alpha"));
        assert!(!encoded.contains("beta"));
    }

    #[test]
    fn an_extra_operation_stays_unauthorised() {
        let mut authorisation = grant();
        authorisation.operations.push("write_file".to_string());
        let report = declare(&AuthoritySource {
            currents: vec![claim(&["preflight"])],
            refusals: Vec::new(),
            grant: Some(authorisation),
        })
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.contexts.len(), 1);
        assert_eq!(report.contexts[0].binding, "owner");
        assert!(report
            .unauthorised
            .iter()
            .any(|row| row.reason == "authority escalation"));
    }

    #[test]
    fn a_source_agent_cannot_become_the_consumer_principal() {
        let mut authorisation = grant();
        authorisation.principal = "preflight".to_string();
        let report = declare(&AuthoritySource {
            currents: vec![claim(&["preflight"])],
            refusals: Vec::new(),
            grant: Some(authorisation),
        })
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.contexts.len(), 1);
        assert_eq!(
            report.unauthorised[0].reason,
            "principal repeats a source agent"
        );
    }

    #[test]
    fn an_empty_journal_passes() {
        let report = declare(&AuthoritySource {
            currents: Vec::new(),
            refusals: Vec::new(),
            grant: None,
        })
        .unwrap();
        assert!(report.pass);
        assert!(!report.historical_permission);
        assert!(report.contexts.is_empty());
    }

    #[test]
    fn a_repeated_current_is_an_error() {
        let current = claim(&["preflight"]);
        let error = declare(&AuthoritySource {
            currents: vec![current.clone(), current],
            refusals: Vec::new(),
            grant: None,
        })
        .unwrap_err();
        assert_eq!(
            error,
            AuthorityError::Message("repeated current tenant-a:read".to_string())
        );
    }
}
