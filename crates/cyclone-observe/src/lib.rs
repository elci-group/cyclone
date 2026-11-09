//! Ingest agent journals and normalise them into causal traces.
//!
//! Order is `(tenant_id, execution_id, sequence)`. Wall-clock timestamps are
//! stored and are not used to sort or to invent parent spans.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const LUCID_SESSION_V1: &str = "lucid.session/v1";
pub const CYCLONE_OBSERVATION_V0: &str = "cyclone.observation/v0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObserveError {
    #[error("line {line}: invalid JSON")]
    Json { line: usize },
    #[error("line {line}: {message}")]
    Line { line: usize, message: String },
    #[error("{message}")]
    Causal { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    ReadOnly,
    FilesystemMutation,
    ExternalMutation,
    Unknown,
    Control,
    Generative,
    Pure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Success,
    Rejected,
    Failed,
    Denied,
    Unclassified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    Agent,
    Mechanical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionObservation {
    pub observation_id: String,
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub agent_id: String,
    pub tenant_id: String,
    pub action_kind: String,
    pub input_digest: Option<String>,
    pub output_digest: Option<String>,
    pub environment_digest: Option<String>,
    pub started_at: String,
    pub completed_at: String,
    pub effect_class: EffectClass,
    pub outcome: ActionOutcome,
    pub evidence_refs: Vec<String>,
    pub sequence: u64,
    pub execution_id: String,
    pub source: ObservationSource,
    pub generative: bool,
}

impl ActionObservation {
    pub fn new(
        execution_id: impl Into<String>,
        sequence: u64,
        action_kind: impl Into<String>,
    ) -> Self {
        let execution_id = execution_id.into();
        let action_kind = action_kind.into();
        let (effect_class, generative) = effect_of(&action_kind);
        Self {
            observation_id: format!("{execution_id}-{sequence}"),
            trace_id: execution_id.clone(),
            parent_span_id: None,
            agent_id: "agent".to_string(),
            tenant_id: "tenant".to_string(),
            action_kind,
            input_digest: None,
            output_digest: None,
            environment_digest: None,
            started_at: "2026-11-08T00:00:00Z".to_string(),
            completed_at: "2026-11-08T00:00:01Z".to_string(),
            effect_class,
            outcome: ActionOutcome::Success,
            evidence_refs: Vec::new(),
            sequence,
            execution_id,
            source: ObservationSource::Agent,
            generative,
        }
    }

    pub fn with_digests(mut self, input: &str, output: &str, environment: &str) -> Self {
        self.input_digest = Some(input.to_string());
        self.output_digest = Some(output.to_string());
        self.environment_digest = Some(environment.to_string());
        self
    }

    pub fn mechanical(mut self) -> Self {
        self.source = ObservationSource::Mechanical;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub trace_id: String,
    pub tenant_id: String,
    pub execution_id: String,
    pub source: ObservationSource,
    pub observations: Vec<ActionObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ingested {
    pub traces: Vec<Trace>,
    pub duplicates_collapsed: u64,
    pub lines: u64,
}

pub fn ingest_jsonl(text: &str) -> Result<Ingested, ObserveError> {
    let mut parsed = Vec::new();
    let mut lines = 0_u64;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        lines += 1;
        let value: Value =
            serde_json::from_str(trimmed).map_err(|_| ObserveError::Json { line: line_no })?;
        let schema = value
            .get("schema_version")
            .and_then(Value::as_str)
            .ok_or_else(|| line_err(line_no, "schema_version is required"))?;
        let observation = match schema {
            LUCID_SESSION_V1 => parse_lucid(line_no, &value)?,
            CYCLONE_OBSERVATION_V0 => parse_cyclone(line_no, &value)?,
            other => {
                return Err(line_err(
                    line_no,
                    format!("unsupported schema_version {other}"),
                ))
            }
        };
        parsed.push(observation);
    }
    let mut ingested = normalise(parsed)?;
    ingested.lines = lines;
    Ok(ingested)
}

fn normalise(observations: Vec<ActionObservation>) -> Result<Ingested, ObserveError> {
    let mut seen: BTreeMap<(String, String, String), ActionObservation> = BTreeMap::new();
    let mut duplicates_collapsed = 0_u64;
    for observation in observations {
        let key = (
            observation.tenant_id.clone(),
            observation.execution_id.clone(),
            observation.observation_id.clone(),
        );
        if let Some(previous) = seen.get(&key) {
            if previous != &observation {
                return Err(ObserveError::Causal {
                    message: format!(
                        "observation {} in execution {} repeats with different content",
                        observation.observation_id, observation.execution_id
                    ),
                });
            }
            duplicates_collapsed += 1;
            continue;
        }
        seen.insert(key, observation);
    }

    let mut grouped: BTreeMap<(String, String), Vec<ActionObservation>> = BTreeMap::new();
    for ((tenant, execution, _), observation) in seen {
        grouped
            .entry((tenant, execution))
            .or_default()
            .push(observation);
    }

    let mut traces = Vec::with_capacity(grouped.len());
    for ((tenant, execution), mut rows) in grouped {
        rows.sort_by(|left, right| left.sequence.cmp(&right.sequence));
        let mut sequences = BTreeMap::<u64, String>::new();
        for row in &rows {
            if let Some(existing) = sequences.insert(row.sequence, row.observation_id.clone()) {
                return Err(ObserveError::Causal {
                    message: format!(
                        "execution {execution} assigns sequence {} to {} and {}",
                        row.sequence, existing, row.observation_id
                    ),
                });
            }
        }
        let trace_id = rows[0].trace_id.clone();
        let source = rows[0].source;
        if rows.iter().any(|row| row.trace_id != trace_id) {
            return Err(ObserveError::Causal {
                message: format!("execution {execution} contains more than one trace_id"),
            });
        }
        if rows
            .iter()
            .any(|row| row.source != source || row.tenant_id != tenant)
        {
            return Err(ObserveError::Causal {
                message: format!("execution {execution} mixes tenant or source"),
            });
        }
        traces.push(Trace {
            trace_id,
            tenant_id: tenant,
            execution_id: execution,
            source,
            observations: rows,
        });
    }

    Ok(Ingested {
        traces,
        duplicates_collapsed,
        lines: 0,
    })
}

fn parse_lucid(line: usize, value: &Value) -> Result<ActionObservation, ObserveError> {
    #[derive(Deserialize)]
    struct Line {
        event_id: String,
        tenant_id: String,
        execution_id: String,
        sequence: u64,
        occurred_at: String,
        kind: String,
        actor_id: Option<String>,
        agent: Option<Agent>,
        detail: Value,
        #[serde(default)]
        payload_sha256: Option<String>,
    }
    #[derive(Deserialize)]
    struct Agent {
        role: Option<String>,
    }

    let parsed: Line = serde_json::from_value(value.clone())
        .map_err(|error| line_err(line, format!("lucid session line is incomplete ({error})")))?;
    require_token(line, "event_id", &parsed.event_id)?;
    require_token(line, "tenant_id", &parsed.tenant_id)?;
    require_token(line, "execution_id", &parsed.execution_id)?;
    require_sequence(line, parsed.sequence)?;
    require_timestamp(line, "occurred_at", &parsed.occurred_at)?;

    let tool = parsed.detail.get("tool").and_then(Value::as_str);
    let status = parsed
        .detail
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (action_kind, outcome, effect_class, generative) =
        map_lucid_kind(line, &parsed.kind, tool, status)?;

    let agent_id = parsed
        .actor_id
        .filter(|value| !value.is_empty())
        .or_else(|| parsed.agent.and_then(|agent| agent.role))
        .ok_or_else(|| line_err(line, "agent identity is missing"))?;
    require_token(line, "agent_id", &agent_id)?;
    require_token(line, "action_kind", &action_kind)?;
    check_effect(line, &action_kind, generative, effect_class)?;

    let mut evidence_refs = Vec::new();
    if let Some(payload) = parsed.payload_sha256 {
        check_sha256(line, "payload_sha256", &payload)?;
        evidence_refs.push(payload);
    }

    Ok(ActionObservation {
        observation_id: parsed.event_id,
        trace_id: parsed.execution_id.clone(),
        parent_span_id: None,
        agent_id,
        tenant_id: parsed.tenant_id,
        action_kind,
        input_digest: None,
        output_digest: None,
        environment_digest: None,
        started_at: parsed.occurred_at.clone(),
        completed_at: parsed.occurred_at,
        effect_class,
        outcome,
        evidence_refs,
        sequence: parsed.sequence,
        execution_id: parsed.execution_id,
        source: ObservationSource::Agent,
        generative,
    })
}

fn parse_cyclone(line: usize, value: &Value) -> Result<ActionObservation, ObserveError> {
    #[derive(Deserialize)]
    struct Row {
        observation_id: String,
        trace_id: String,
        parent_span_id: Option<String>,
        agent_id: String,
        tenant_id: String,
        execution_id: String,
        sequence: u64,
        action_kind: String,
        input_digest: Option<String>,
        output_digest: Option<String>,
        environment_digest: Option<String>,
        started_at: String,
        completed_at: String,
        effect_class: EffectClass,
        outcome: ActionOutcome,
        #[serde(default)]
        evidence_refs: Vec<String>,
        source: ObservationSource,
        generative: bool,
    }

    let row: Row = serde_json::from_value(value.clone())
        .map_err(|error| line_err(line, format!("cyclone observation is incomplete ({error})")))?;
    for (field, value) in [
        ("observation_id", row.observation_id.as_str()),
        ("trace_id", row.trace_id.as_str()),
        ("agent_id", row.agent_id.as_str()),
        ("tenant_id", row.tenant_id.as_str()),
        ("execution_id", row.execution_id.as_str()),
        ("action_kind", row.action_kind.as_str()),
    ] {
        require_token(line, field, value)?;
    }
    if let Some(parent) = &row.parent_span_id {
        require_token(line, "parent_span_id", parent)?;
    }
    require_sequence(line, row.sequence)?;
    require_timestamp(line, "started_at", &row.started_at)?;
    require_timestamp(line, "completed_at", &row.completed_at)?;
    if timestamp_order_key(&row.started_at) > timestamp_order_key(&row.completed_at) {
        return Err(line_err(line, "completed_at precedes started_at"));
    }
    for (field, digest) in [
        ("input_digest", &row.input_digest),
        ("output_digest", &row.output_digest),
        ("environment_digest", &row.environment_digest),
    ] {
        if let Some(digest) = digest {
            check_prefixed_digest(line, field, digest)?;
        }
    }
    check_effect(line, &row.action_kind, row.generative, row.effect_class)?;
    Ok(ActionObservation {
        observation_id: row.observation_id,
        trace_id: row.trace_id,
        parent_span_id: row.parent_span_id,
        agent_id: row.agent_id,
        tenant_id: row.tenant_id,
        action_kind: row.action_kind,
        input_digest: row.input_digest,
        output_digest: row.output_digest,
        environment_digest: row.environment_digest,
        started_at: row.started_at,
        completed_at: row.completed_at,
        effect_class: row.effect_class,
        outcome: row.outcome,
        evidence_refs: row.evidence_refs,
        sequence: row.sequence,
        execution_id: row.execution_id,
        source: row.source,
        generative: row.generative,
    })
}

fn map_lucid_kind(
    line: usize,
    kind: &str,
    tool: Option<&str>,
    status: &str,
) -> Result<(String, ActionOutcome, EffectClass, bool), ObserveError> {
    match kind {
        "model.response" => Ok((
            "model.response".to_string(),
            ActionOutcome::Success,
            EffectClass::Generative,
            true,
        )),
        "tool.completed" => {
            let action = tool
                .ok_or_else(|| line_err(line, "tool.completed needs detail.tool"))?
                .to_string();
            let outcome = match status {
                "applied" | "success" => ActionOutcome::Success,
                "rejected" => ActionOutcome::Rejected,
                "failed" => ActionOutcome::Failed,
                "" => return Err(line_err(line, "tool.completed needs detail.status")),
                other => return Err(line_err(line, format!("unsupported tool status {other}"))),
            };
            let (effect, _) = effect_of(&action);
            Ok((action, outcome, effect, false))
        }
        "policy.denied" => {
            let action = tool.unwrap_or("policy.denied").to_string();
            let (effect, _) = effect_of(&action);
            Ok((action, ActionOutcome::Denied, effect, false))
        }
        "task.running" | "task.completed" | "message.applied" | "finish" => Ok((
            kind.to_string(),
            ActionOutcome::Success,
            EffectClass::Control,
            false,
        )),
        _ => Ok((
            kind.to_string(),
            ActionOutcome::Unclassified,
            EffectClass::Unknown,
            false,
        )),
    }
}

fn effect_of(action_kind: &str) -> (EffectClass, bool) {
    match action_kind {
        "model.response" => (EffectClass::Generative, true),
        "read" => (EffectClass::ReadOnly, false),
        "pure" => (EffectClass::Pure, false),
        "write_file" | "write_worktree" | "propose_patch" | "apply_source" | "delete_file" => {
            (EffectClass::FilesystemMutation, false)
        }
        "run_checks" => (EffectClass::Unknown, false),
        "message.applied" | "task.running" | "task.completed" | "finish" => {
            (EffectClass::Control, false)
        }
        _ => (EffectClass::Unknown, false),
    }
}

fn check_effect(
    line: usize,
    action_kind: &str,
    generative: bool,
    effect_class: EffectClass,
) -> Result<(), ObserveError> {
    if generative != (action_kind == "model.response") {
        return Err(line_err(line, "generative is reserved for model.response"));
    }
    let (expected, _) = effect_of(action_kind);
    let known = !matches!(
        action_kind,
        "model.response"
            | "read"
            | "pure"
            | "write_file"
            | "write_worktree"
            | "propose_patch"
            | "apply_source"
            | "delete_file"
            | "run_checks"
            | "message.applied"
            | "task.running"
            | "task.completed"
            | "finish"
    ) && expected == EffectClass::Unknown
        && !generative;
    if known {
        if effect_class == EffectClass::Generative || effect_class == EffectClass::Control {
            return Err(line_err(
                line,
                format!("{action_kind} cannot be declared {effect_class:?}"),
            ));
        }
        return Ok(());
    }
    if effect_class != expected {
        return Err(line_err(
            line,
            format!("{action_kind} effect does not match the declared class"),
        ));
    }
    Ok(())
}

fn check_prefixed_digest(line: usize, field: &str, value: &str) -> Result<(), ObserveError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(line_err(line, format!("{field} must be sha256:<64 hex>")));
    };
    check_sha256(line, field, hex)
}

fn check_sha256(line: usize, field: &str, hex: &str) -> Result<(), ObserveError> {
    if hex.len() == 64 && hex.chars().all(|character| character.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(line_err(line, format!("{field} must be 64 hex characters")))
    }
}

fn require_token(line: usize, field: &str, value: &str) -> Result<(), ObserveError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_whitespace) {
        Err(line_err(
            line,
            format!("{field} must be a token of 1 to 128 characters"),
        ))
    } else {
        Ok(())
    }
}

fn require_sequence(line: usize, sequence: u64) -> Result<(), ObserveError> {
    if sequence == 0 {
        Err(line_err(line, "sequence starts at 1"))
    } else {
        Ok(())
    }
}

fn require_timestamp(line: usize, field: &str, value: &str) -> Result<(), ObserveError> {
    if !is_rfc3339(value) {
        return Err(line_err(
            line,
            format!("{field} must be an RFC 3339 timestamp"),
        ));
    }
    Ok(())
}

fn is_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b't'))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let number = |start: usize, end: usize| -> Option<u32> {
        let slice = bytes.get(start..end)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(slice).ok()?.parse().ok()
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0, 4),
        number(5, 7),
        number(8, 10),
        number(11, 13),
        number(14, 16),
        number(17, 19),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 60 {
        return false;
    }
    let mut pos = 19;
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if pos == start {
            return false;
        }
    }
    match bytes.get(pos) {
        Some(b'Z' | b'z') => pos + 1 == bytes.len(),
        Some(b'+' | b'-') => {
            pos + 6 == bytes.len()
                && bytes.get(pos + 3) == Some(&b':')
                && bytes
                    .get(pos + 1..pos + 3)
                    .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
                && bytes
                    .get(pos + 4..pos + 6)
                    .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
                && number(pos + 1, pos + 3).is_some_and(|v| v <= 23)
                && number(pos + 4, pos + 6).is_some_and(|v| v <= 59)
        }
        _ => false,
    }
}

fn timestamp_order_key(value: &str) -> (i64, String) {
    let bytes = value.as_bytes();
    let offset_pos = bytes
        .iter()
        .enumerate()
        .skip(19)
        .find_map(|(i, b)| matches!(b, b'Z' | b'z' | b'+' | b'-').then_some(i))
        .unwrap_or(value.len());
    let frac = value.get(19..offset_pos).unwrap_or("");
    let fraction = frac
        .strip_prefix('.')
        .unwrap_or("")
        .trim_end_matches('0')
        .to_string();
    let offset = value.get(offset_pos..).unwrap_or("Z");
    let (sign, oh, om) = if offset == "Z" || offset == "z" {
        (0_i64, 0_i64, 0_i64)
    } else {
        let sign = if offset.starts_with('+') { 1 } else { -1 };
        let h = offset
            .get(1..3)
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let m = offset
            .get(4..6)
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        (sign, h, m)
    };
    let local = value.get(..19).unwrap_or(value);
    let (year, month, day, hour, minute, second) = (
        number_at(local, 0, 4),
        number_at(local, 5, 7),
        number_at(local, 8, 10),
        number_at(local, 11, 13),
        number_at(local, 14, 16),
        number_at(local, 17, 19),
    );
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146097 + day_of_era;
    let utc_seconds =
        days * 86400 + hour * 3600 + minute * 60 + second - sign * (oh * 3600 + om * 60);
    (utc_seconds, fraction)
}

fn number_at(value: &str, start: usize, end: usize) -> i64 {
    value
        .get(start..end)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn line_err(line: usize, message: impl Into<String>) -> ObserveError {
    ObserveError::Line {
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lucid_fixture_orders_by_sequence_and_collapses_the_duplicate() {
        let text = include_str!("../../../traces/lucid-session-v1.jsonl");
        let ingested = ingest_jsonl(text).unwrap();
        assert_eq!(ingested.duplicates_collapsed, 1);
        assert_eq!(ingested.traces.len(), 1);
        let trace = &ingested.traces[0];
        let kinds: Vec<&str> = trace
            .observations
            .iter()
            .map(|row| row.action_kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "task.running",
                "model.response",
                "write_file",
                "write_file",
                "delete_file",
                "message.applied",
                "task.completed",
            ]
        );
        assert_eq!(
            trace
                .observations
                .iter()
                .map(|row| row.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6, 7]
        );
        assert!(trace.observations[1].generative);
        assert_eq!(trace.observations[1].effect_class, EffectClass::Generative);
        assert_eq!(trace.observations[2].outcome, ActionOutcome::Rejected);
        assert_eq!(trace.observations[4].outcome, ActionOutcome::Denied);
        assert!(trace
            .observations
            .iter()
            .all(|row| row.parent_span_id.is_none()));
        assert!(trace
            .observations
            .iter()
            .all(|row| row.input_digest.is_none()));
    }

    #[test]
    fn timestamps_do_not_reorder_events() {
        let text = r#"
{"schema_version":"cyclone.observation/v0","observation_id":"b","trace_id":"t","parent_span_id":null,"agent_id":"agent","tenant_id":"tenant","execution_id":"exec","sequence":2,"action_kind":"read","input_digest":null,"output_digest":null,"environment_digest":null,"started_at":"2026-11-08T00:00:00Z","completed_at":"2026-11-08T00:00:01Z","effect_class":"read_only","outcome":"success","evidence_refs":[],"source":"agent","generative":false}
{"schema_version":"cyclone.observation/v0","observation_id":"a","trace_id":"t","parent_span_id":null,"agent_id":"agent","tenant_id":"tenant","execution_id":"exec","sequence":1,"action_kind":"read","input_digest":null,"output_digest":null,"environment_digest":null,"started_at":"2026-11-08T00:00:09Z","completed_at":"2026-11-08T00:00:10Z","effect_class":"read_only","outcome":"success","evidence_refs":[],"source":"agent","generative":false}
"#;
        let ingested = ingest_jsonl(text).unwrap();
        let ids: Vec<&str> = ingested.traces[0]
            .observations
            .iter()
            .map(|row| row.observation_id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn timestamp_validation_checks_calendar_and_offsets() {
        assert!(is_rfc3339("2024-02-29T23:59:60.123Z"));
        assert!(is_rfc3339("2026-11-08T00:00:00+01:30"));
        assert!(!is_rfc3339("2025-02-29T00:00:00Z"));
        assert!(!is_rfc3339("2026-11-08 00:00:00"));
        assert!(!is_rfc3339("2026-11-08T00:00:00+24:00"));
    }

    #[test]
    fn observation_intervals_compare_across_timezones() {
        assert!(
            timestamp_order_key("2026-11-08T10:00:00+01:00")
                < timestamp_order_key("2026-11-08T09:30:00Z")
        );
        assert!(
            timestamp_order_key("2026-11-08T10:00:00Z")
                > timestamp_order_key("2026-11-08T09:59:59.999Z")
        );
        assert_eq!(
            timestamp_order_key("2026-11-08T09:00:00.10Z"),
            timestamp_order_key("2026-11-08T09:00:00.1Z")
        );
    }

    #[test]
    fn a_pure_action_ingests_as_pure() {
        let text = r#"{"schema_version":"cyclone.observation/v0","observation_id":"p","trace_id":"t","parent_span_id":null,"agent_id":"agent","tenant_id":"tenant","execution_id":"exec","sequence":1,"action_kind":"pure","input_digest":null,"output_digest":null,"environment_digest":null,"started_at":"2026-11-08T00:00:00Z","completed_at":"2026-11-08T00:00:01Z","effect_class":"pure","outcome":"success","evidence_refs":[],"source":"agent","generative":false}"#;
        let ingested = ingest_jsonl(text).unwrap();
        let observation = &ingested.traces[0].observations[0];
        assert_eq!(observation.action_kind, "pure");
        assert_eq!(observation.effect_class, EffectClass::Pure);
        assert!(!observation.generative);
    }

    #[test]
    fn conflicting_duplicate_is_rejected() {
        let text = r#"
{"schema_version":"cyclone.observation/v0","observation_id":"a","trace_id":"t","parent_span_id":null,"agent_id":"agent","tenant_id":"tenant","execution_id":"exec","sequence":1,"action_kind":"read","input_digest":null,"output_digest":null,"environment_digest":null,"started_at":"2026-11-08T00:00:00Z","completed_at":"2026-11-08T00:00:01Z","effect_class":"read_only","outcome":"success","evidence_refs":[],"source":"agent","generative":false}
{"schema_version":"cyclone.observation/v0","observation_id":"a","trace_id":"t","parent_span_id":null,"agent_id":"agent","tenant_id":"tenant","execution_id":"exec","sequence":1,"action_kind":"read","input_digest":null,"output_digest":null,"environment_digest":null,"started_at":"2026-11-08T00:00:00Z","completed_at":"2026-11-08T00:00:02Z","effect_class":"read_only","outcome":"success","evidence_refs":[],"source":"agent","generative":false}
"#;
        let error = ingest_jsonl(text).unwrap_err();
        assert!(matches!(error, ObserveError::Causal { .. }));
    }
}
