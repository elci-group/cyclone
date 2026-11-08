//! Execution guarantees for compiled currents.
//!
//! A pure current may be retried within a limit of three attempts. A
//! read-only current follows a policy of one attempt. The journal performs
//! one lookup. Exactly-once stays unclaimed.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use serde::Serialize;
use thiserror::Error;

pub const GUARANTEES_V0: &str = "cyclone.guarantees/v0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GuaranteeError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuaranteeSource {
    pub currents: Vec<CurrentClaim>,
    pub refusals: Vec<RefusalClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentClaim {
    pub current_id: String,
    pub signature: Vec<String>,
    pub effects: Vec<String>,
    pub idempotency: String,
    pub max_attempts: u32,
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
pub struct GuaranteeReport {
    pub schema: String,
    pub pass: bool,
    pub exactly_once: bool,
    pub transactional: bool,
    pub guarantees: Vec<Guarantee>,
    pub unclaimed: Vec<Unclaimed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Guarantee {
    pub current_id: String,
    pub signature: Vec<String>,
    pub category: String,
    pub kind: String,
    pub attempts_limit: u32,
    pub attempts_performed: u32,
    pub exactly_once: bool,
    pub operation_id: String,
    pub journal: String,
    pub reconciliation: bool,
    pub transactional: bool,
    pub shadow_pass: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unclaimed {
    pub signature: Vec<String>,
    pub reason: String,
    pub exactly_once: bool,
}

pub fn declare(source: &GuaranteeSource) -> Result<GuaranteeReport, GuaranteeError> {
    let mut seen = BTreeSet::new();
    let mut guarantees = Vec::new();
    let mut unclaimed = Vec::new();
    for refusal in &source.refusals {
        unclaimed.push(Unclaimed {
            signature: refusal.signature.clone(),
            reason: refusal.reason.clone(),
            exactly_once: false,
        });
    }
    for current in &source.currents {
        if !seen.insert(current.current_id.as_str()) {
            return Err(GuaranteeError::Message(format!(
                "repeated current {}",
                current.current_id
            )));
        }
        if current.effects.is_empty() {
            return Err(GuaranteeError::Message(format!(
                "effects missing {}",
                current.current_id
            )));
        }
        match policy_of(&current.effects) {
            Some((category, kind, limit, idempotency)) => {
                if current.idempotency != idempotency {
                    return Err(GuaranteeError::Message(format!(
                        "idempotency does not match {}",
                        current.current_id
                    )));
                }
                if current.max_attempts != limit {
                    return Err(GuaranteeError::Message(format!(
                        "attempts do not match {}",
                        current.current_id
                    )));
                }
                let held = current.shadow_checked > 0
                    && current.shadow_matched == current.shadow_checked
                    && current.shadow_pass;
                guarantees.push(Guarantee {
                    current_id: current.current_id.clone(),
                    signature: current.signature.clone(),
                    category: category.to_string(),
                    kind: kind.to_string(),
                    attempts_limit: limit,
                    attempts_performed: 1,
                    exactly_once: false,
                    operation_id: current.current_id.clone(),
                    journal: "memory".to_string(),
                    reconciliation: false,
                    transactional: false,
                    shadow_pass: held,
                });
            }
            None => unclaimed.push(Unclaimed {
                signature: current.signature.clone(),
                reason: "exactly-once unclaimed".to_string(),
                exactly_once: false,
            }),
        }
    }
    let pass = guarantees.iter().all(|row| {
        !row.exactly_once
            && !row.transactional
            && !row.reconciliation
            && row.journal == "memory"
            && row.attempts_performed == 1
            && row.attempts_performed <= row.attempts_limit
            && row.operation_id == row.current_id
            && row.shadow_pass
            && kind_matches(&row.category, &row.kind, row.attempts_limit)
    }) && unclaimed.iter().all(|row| !row.exactly_once);
    Ok(GuaranteeReport {
        schema: GUARANTEES_V0.to_string(),
        pass,
        exactly_once: false,
        transactional: false,
        guarantees,
        unclaimed,
    })
}

fn policy_of(effects: &[String]) -> Option<(&'static str, &'static str, u32, &'static str)> {
    if effects
        .iter()
        .any(|effect| effect != "pure" && effect != "read_only")
    {
        return None;
    }
    if effects.iter().any(|effect| effect == "read_only") {
        Some(("read_only", "policy_retry", 1, "read_only"))
    } else {
        Some(("pure", "retry_within_limit", 3, "pure"))
    }
}

fn kind_matches(category: &str, kind: &str, limit: u32) -> bool {
    match category {
        "pure" => kind == "retry_within_limit" && limit == 3,
        "read_only" => kind == "policy_retry" && limit == 1,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(effects: &[&str], matched: u64, checked: u64, pass: bool) -> CurrentClaim {
        let read_only = effects.iter().any(|effect| *effect == "read_only");
        let supported = effects
            .iter()
            .all(|effect| *effect == "pure" || *effect == "read_only");
        let (idempotency, max_attempts) = if !supported {
            ("external", 1)
        } else if read_only {
            ("read_only", 1)
        } else {
            ("pure", 3)
        };
        CurrentClaim {
            current_id: "tenant-a:read".to_string(),
            signature: vec!["read".to_string()],
            effects: effects.iter().map(|effect| (*effect).to_string()).collect(),
            idempotency: idempotency.to_string(),
            max_attempts,
            shadow_pass: pass,
            shadow_checked: checked,
            shadow_matched: matched,
        }
    }

    #[test]
    fn a_pure_current_stays_inside_three_attempts() {
        let report = declare(&GuaranteeSource {
            currents: vec![claim(&["pure"], 30, 30, true)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(report.pass);
        assert!(!report.exactly_once);
        assert!(!report.transactional);
        assert_eq!(report.guarantees[0].category, "pure");
        assert_eq!(report.guarantees[0].kind, "retry_within_limit");
        assert_eq!(report.guarantees[0].attempts_limit, 3);
        assert_eq!(report.guarantees[0].attempts_performed, 1);
        assert!(!report.guarantees[0].exactly_once);
        assert_eq!(report.guarantees[0].journal, "memory");
        assert!(!report.guarantees[0].reconciliation);
        assert_eq!(report.guarantees[0].operation_id, "tenant-a:read");
    }

    #[test]
    fn a_read_only_current_follows_a_one_attempt_policy() {
        let report = declare(&GuaranteeSource {
            currents: vec![claim(&["read_only"], 30, 30, true)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(report.pass);
        assert_eq!(report.guarantees[0].category, "read_only");
        assert_eq!(report.guarantees[0].kind, "policy_retry");
        assert_eq!(report.guarantees[0].attempts_limit, 1);
        assert_eq!(report.guarantees[0].attempts_performed, 1);
        assert!(report.guarantees[0].shadow_pass);
    }

    #[test]
    fn a_mixed_pure_step_uses_the_read_only_policy() {
        let report = declare(&GuaranteeSource {
            currents: vec![claim(&["pure", "read_only"], 2, 2, true)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert_eq!(report.guarantees[0].category, "read_only");
        assert_eq!(report.guarantees[0].kind, "policy_retry");
        assert_eq!(report.guarantees[0].attempts_limit, 1);
    }

    #[test]
    fn a_filesystem_effect_stays_unclaimed() {
        let report = declare(&GuaranteeSource {
            currents: vec![claim(&["filesystem_mutation"], 1, 1, true)],
            refusals: vec![RefusalClaim {
                signature: vec!["write_file".to_string()],
                reason: "effect class is not pure or read_only".to_string(),
            }],
        })
        .unwrap();
        assert!(report.pass);
        assert!(report.guarantees.is_empty());
        assert!(!report.exactly_once);
        assert_eq!(report.unclaimed.len(), 2);
        assert_eq!(report.unclaimed[1].reason, "exactly-once unclaimed");
        assert!(!report.unclaimed[1].exactly_once);
        let value = serde_json::to_value(&report).unwrap();
        assert!(value.get("prompt").is_none());
        assert!(value.get("secret").is_none());
    }

    #[test]
    fn a_failed_shadow_fails_the_declared_guarantee() {
        let report = declare(&GuaranteeSource {
            currents: vec![claim(&["pure"], 29, 30, false)],
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(!report.pass);
        assert!(!report.guarantees[0].shadow_pass);
        assert!(!report.guarantees[0].exactly_once);
        assert_eq!(report.guarantees[0].attempts_performed, 1);
        assert_eq!(report.schema, "cyclone.guarantees/v0");
    }

    #[test]
    fn an_empty_journal_passes() {
        let report = declare(&GuaranteeSource {
            currents: Vec::new(),
            refusals: Vec::new(),
        })
        .unwrap();
        assert!(report.pass);
        assert!(!report.exactly_once);
        assert!(report.guarantees.is_empty());
        assert!(report.unclaimed.is_empty());
    }

    #[test]
    fn a_repeated_current_is_an_error() {
        let current = claim(&["pure"], 1, 1, true);
        let error = declare(&GuaranteeSource {
            currents: vec![current.clone(), current],
            refusals: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(
            error,
            GuaranteeError::Message("repeated current tenant-a:read".to_string())
        );
    }

    #[test]
    fn a_wider_attempt_limit_is_refused() {
        let mut current = claim(&["pure"], 1, 1, true);
        current.max_attempts = 4;
        let error = declare(&GuaranteeSource {
            currents: vec![current],
            refusals: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(
            error,
            GuaranteeError::Message("attempts do not match tenant-a:read".to_string())
        );
    }

    #[test]
    fn a_mismatched_idempotency_is_refused() {
        let mut current = claim(&["read_only"], 1, 1, true);
        current.idempotency = "pure".to_string();
        let error = declare(&GuaranteeSource {
            currents: vec![current],
            refusals: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(
            error,
            GuaranteeError::Message("idempotency does not match tenant-a:read".to_string())
        );
    }
}
