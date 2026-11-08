//! Admission telemetry.
//!
//! A decision records the route taken for one invocation. A drift signal
//! records why a current left its route. Payloads, prompts, paths, and
//! secrets stay out of both records.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const TELEMETRY_V0: &str = "cyclone.telemetry/v0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftSignal {
    pub kind: String,
    pub current_id: String,
    pub execution_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub execution_id: String,
    pub current_id: String,
    pub tenant_id: String,
    pub lifecycle: String,
    pub pressure: String,
    pub route: String,
    pub outcome: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Telemetry {
    pub schema: String,
    pub decisions: Vec<Decision>,
    pub drifts: Vec<DriftSignal>,
}

impl Telemetry {
    pub fn empty() -> Self {
        Self {
            schema: TELEMETRY_V0.to_string(),
            decisions: Vec::new(),
            drifts: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decision_serializes_without_a_prompt() {
        let decision = Decision {
            execution_id: "exec-01".to_string(),
            current_id: "tenant-a:read".to_string(),
            tenant_id: "tenant-a".to_string(),
            lifecycle: "shadow".to_string(),
            pressure: "high".to_string(),
            route: "original_agent_workflow".to_string(),
            outcome: "matched".to_string(),
            reason: "shadow compare".to_string(),
        };
        let value = serde_json::to_value(&decision).unwrap();
        let object = value.as_object().unwrap();
        assert!(object.get("prompt").is_none());
        assert!(object.get("path").is_none());
        assert!(object.get("secret").is_none());
        let telemetry = Telemetry {
            schema: TELEMETRY_V0.to_string(),
            decisions: vec![decision],
            drifts: Vec::new(),
        };
        assert_eq!(telemetry.schema, "cyclone.telemetry/v0");
    }
}
