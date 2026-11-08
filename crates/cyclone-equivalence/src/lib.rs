//! Equivalence contracts for compiled currents.
//!
//! A pure current declares exact output equality. A read-only current
//! declares equality under one environment snapshot. Other effect classes
//! stay undeclared. Irreversible replay stays false.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use serde::Serialize;
use thiserror::Error;

pub const EQUIVALENCE_V0: &str = "cyclone.equivalence/v0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EquivalenceError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquivalenceSource {
    pub currents: Vec<CurrentClaim>,
    pub refusals: Vec<RefusalClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentClaim {
    pub current_id: String,
    pub signature: Vec<String>,
    pub effects: Vec<String>,
    pub shadow_pass: bool,
    pub shadow_checked: u64,
    pub shadow_matched: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalClaim {
    pub signature: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EquivalenceReport {
    pub schema: String,
    pub pass: bool,
    pub supported: Vec<String>,
    pub contracts: Vec<Contract>,
    pub undeclared: Vec<Undeclared>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Contract {
    pub current_id: String,
    pub signature: Vec<String>,
    pub category: String,
    pub check: String,
    pub tolerance: String,
    pub shadow_pass: bool,
    pub shadow_checked: u64,
    pub shadow_matched: u64,
    pub irreversible_replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Undeclared {
    pub signature: Vec<String>,
    pub reason: String,
}

pub fn declare(source: &EquivalenceSource) -> Result<EquivalenceReport, EquivalenceError> {
    let mut seen = BTreeSet::new();
    let mut contracts = Vec::new();
    let mut undeclared = Vec::new();
    for refusal in &source.refusals {
        undeclared.push(Undeclared {
            signature: refusal.signature.clone(),
            reason: refusal.reason.clone(),
        });
    }
    for current in &source.currents {
        if !seen.insert(current.current_id.as_str()) {
            return Err(EquivalenceError::Message(format!(
                "repeated current {}",
                current.current_id
            )));
        }
        if current.effects.is_empty() {
            return Err(EquivalenceError::Message(format!(
                "effects missing {}",
                current.current_id
            )));
        }
        match category_of(&current.effects) {
            Some((category, check)) => {
                let held = current.shadow_checked > 0
                    && current.shadow_matched == current.shadow_checked
                    && current.shadow_pass;
                contracts.push(Contract {
                    current_id: current.current_id.clone(),
                    signature: current.signature.clone(),
                    category: category.to_string(),
                    check: check.to_string(),
                    tolerance: "none".to_string(),
                    shadow_pass: held,
                    shadow_checked: current.shadow_checked,
                    shadow_matched: current.shadow_matched,
                    irreversible_replay: false,
                });
            }
            None => undeclared.push(Undeclared {
                signature: current.signature.clone(),
                reason: "effect has no equivalence check".to_string(),
            }),
        }
    }
    let pass = contracts.iter().all(|contract| {
        contract.shadow_pass
            && !contract.irreversible_replay
            && contract.tolerance == "none"
            && check_matches(&contract.category, &contract.check)
    });
    Ok(EquivalenceReport {
        schema: EQUIVALENCE_V0.to_string(),
        pass,
        supported: vec!["pure".to_string(), "read_only".to_string()],
        contracts,
        undeclared,
    })
}

fn category_of(effects: &[String]) -> Option<(&'static str, &'static str)> {
    if effects
        .iter()
        .any(|effect| effect != "pure" && effect != "read_only")
    {
        return None;
    }
    if effects.iter().any(|effect| effect == "read_only") {
        Some(("read_only", "environment_snapshot"))
    } else {
        Some(("pure", "exact_output"))
    }
}

fn check_matches(category: &str, check: &str) -> bool {
    match category {
        "pure" => check == "exact_output",
        "read_only" => check == "environment_snapshot",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(effects: &[&str], matched: u64, checked: u64, pass: bool) -> CurrentClaim {
        CurrentClaim {
            current_id: "tenant-a:read".to_string(),
            signature: vec!["read".to_string()],
            effects: effects.iter().map(|effect| (*effect).to_string()).collect(),
            shadow_pass: pass,
            shadow_checked: checked,
            shadow_matched: matched,
        }
    }

    #[test]
    fn a_pure_current_declares_exact_output() {
        let report = declare(&EquivalenceSource {
            currents: vec![claim(&["pure"], 30, 30, true)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.contracts[0].category, "pure");
        assert_eq!(report.contracts[0].check, "exact_output");
        assert_eq!(report.contracts[0].tolerance, "none");
        assert!(!report.contracts[0].irreversible_replay);
        assert_eq!(report.supported, vec!["pure", "read_only"]);
    }

    #[test]
    fn a_read_only_current_declares_the_environment_snapshot() {
        let report = declare(&EquivalenceSource {
            currents: vec![claim(&["read_only"], 30, 30, true)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.contracts[0].category, "read_only");
        assert_eq!(report.contracts[0].check, "environment_snapshot");
        assert_eq!(report.contracts[0].shadow_checked, 30);
        assert_eq!(report.contracts[0].shadow_matched, 30);
    }

    #[test]
    fn a_mixed_pure_step_uses_the_read_only_contract() {
        let report = declare(&EquivalenceSource {
            currents: vec![claim(&["pure", "read_only"], 2, 2, true)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert_eq!(report.contracts[0].category, "read_only");
        assert_eq!(report.contracts[0].check, "environment_snapshot");
    }

    #[test]
    fn a_filesystem_effect_stays_undeclared() {
        let report = declare(&EquivalenceSource {
            currents: vec![claim(&["filesystem_mutation"], 1, 1, true)],
            refusals: vec![RefusalClaim {
                signature: vec!["write_file".to_string()],
                reason: "effect class is not pure or read_only".to_string(),
            }],
        })
        .unwrap();
        assert!(report.pass);
        assert!(report.contracts.is_empty());
        assert_eq!(report.undeclared.len(), 2);
        assert_eq!(
            report.undeclared[1].reason,
            "effect has no equivalence check"
        );
        let value = serde_json::to_value(&report).unwrap();
        assert!(value.get("prompt").is_none());
        assert!(value.get("secret").is_none());
    }

    #[test]
    fn a_failed_shadow_fails_the_declared_contract() {
        let report = declare(&EquivalenceSource {
            currents: vec![claim(&["pure"], 29, 30, false)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(!report.pass);
        assert!(!report.contracts[0].shadow_pass);
        assert_eq!(report.schema, "cyclone.equivalence/v0");
    }

    #[test]
    fn an_empty_journal_passes() {
        let report = declare(&EquivalenceSource {
            currents: Vec::new(),
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(report.pass);
        assert!(report.contracts.is_empty());
    }

    #[test]
    fn a_repeated_current_is_an_error() {
        let current = claim(&["pure"], 1, 1, true);
        let error = declare(&EquivalenceSource {
            currents: vec![current.clone(), current],
            refusals: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(
            error,
            EquivalenceError::Message("repeated current tenant-a:read".to_string())
        );
    }
}
