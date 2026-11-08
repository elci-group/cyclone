use std::path::PathBuf;
use std::process::Command;

fn trace(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../traces")
        .join(name)
}

fn audit(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_cyclone"))
        .args(args)
        .output()
        .expect("run cyclone");
    let text = String::from_utf8(output.stdout).expect("utf8 stdout");
    (output.status.success(), text)
}

#[test]
fn lucid_journal_has_no_compilable_candidate() {
    let (ok, text) = audit(&[
        "audit",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.audit/v0");
    assert_eq!(report["duplicates_collapsed"], 1);
    assert_eq!(report["observations"], 7);
    assert_eq!(report["candidates"], serde_json::json!([]));
}

#[test]
fn repeated_reads_stay_gust_until_evidence_is_attached() {
    let path = trace("reads-30.jsonl");
    let (ok, bare) = audit(&["audit", "--json", path.to_str().unwrap()]);
    assert!(ok, "{bare}");
    let bare: serde_json::Value = serde_json::from_str(&bare).unwrap();
    assert_eq!(bare["candidates"][0]["pressure"], "gust");
    assert_eq!(bare["candidates"][0]["promotion"], "refused");

    let evidence = trace("reads-30.evidence.json");
    let (ok, attached) = audit(&[
        "audit",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert!(ok, "{attached}");
    let attached: serde_json::Value = serde_json::from_str(&attached).unwrap();
    assert_eq!(attached["candidates"][0]["pressure"], "high");
    assert_eq!(attached["candidates"][0]["promotion"], "eligible");
    assert_eq!(attached["candidates"][0]["agent_executions"], 30);
}

#[test]
fn the_same_trace_audits_to_the_same_bytes() {
    let path = trace("reads-30.jsonl");
    let evidence = trace("reads-30.evidence.json");
    let args = [
        "audit",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ];
    let (ok_left, left) = audit(&args);
    let (ok_right, right) = audit(&args);
    assert!(ok_left && ok_right);
    assert_eq!(left, right);
}

#[test]
fn split_environments_stay_refused_with_positive_evidence() {
    let (ok, text) = audit(&[
        "audit",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("environment-split.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["candidates"][0]["promotion"], "refused");
    let explanation = report["candidates"][0]["explanation"].to_string();
    assert!(explanation.contains("environment"), "{explanation}");
}

#[test]
fn mechanical_replicates_do_not_count_as_agent_observations() {
    let (ok, text) = audit(&[
        "audit",
        "--json",
        trace("mechanical-preflight.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["candidates"][0]["agent_executions"], 0);
    assert_eq!(report["candidates"][0]["mechanical_executions"], 6);
    assert_eq!(report["candidates"][0]["promotion"], "refused");
}

#[test]
fn reads_compile_and_verify_when_evidence_is_attached() {
    let path = trace("reads-30.jsonl");
    let evidence = trace("reads-30.evidence.json");
    let (ok, text) = audit(&[
        "compile",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.compile/v0");
    assert_eq!(report["compiled"].as_array().unwrap().len(), 1);
    assert_eq!(report["compiled"][0]["schedule"], "interpret");
    assert_eq!(report["compiled"][0]["current_id"], "tenant-a:read");
    assert!(report["refused"].as_array().unwrap().is_empty());

    let (ok, text) = audit(&[
        "verify",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.verify/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["checked"], 30);
    assert_eq!(report["passed"], 30);
    assert_eq!(report["results"][0]["matched"], 30);
}

#[test]
fn lucid_verify_passes_with_nothing_to_check() {
    let (ok, text) = audit(&[
        "verify",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["checked"], 0);
    assert!(report["compiled"].as_array().unwrap().is_empty());
}

#[test]
fn a_pure_current_verifies() {
    let path = trace("pure-30.jsonl");
    let evidence = trace("pure-30.evidence.json");
    let (ok, text) = audit(&[
        "verify",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["checked"], 30);
    assert_eq!(report["compiled"][0]["idempotency"], "pure");
    assert_eq!(report["compiled"][0]["retry"]["max_attempts"], 3);
}

#[test]
fn a_chain_schedules_in_causal_order() {
    let (ok, text) = audit(&[
        "schedule",
        "--json",
        "--evidence",
        trace("compose-chain.evidence.json").to_str().unwrap(),
        trace("compose-chain.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.schedule/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["graph"]["schema"], "cyclone.graph/v0");
    assert_eq!(report["graph"]["waves"].as_array().unwrap().len(), 3);
    assert_eq!(
        report["graph"]["critical_path"].as_array().unwrap().len(),
        3
    );
    assert_eq!(report["graph"]["edges"][0]["trigger"], "data");
    assert_eq!(report["graph"]["edges"][1]["trigger"], "sequential");
    assert_eq!(
        report["run"]["effects"],
        serde_json::json!(["pure", "read_only", "pure"])
    );
    assert_eq!(report["run"]["evaluations"], 3);
    assert_eq!(
        report["run"]["order"],
        serde_json::json!(["tenant-a:pure", "tenant-a:read", "tenant-a:normalise"])
    );
}

#[test]
fn shared_pure_lookups_evaluate_once() {
    let (ok, text) = audit(&[
        "schedule",
        "--json",
        "--evidence",
        trace("compose-share.evidence.json").to_str().unwrap(),
        trace("compose-share.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["run"]["evaluations"], 1);
    assert!(report["run"]["reuses"].as_u64().unwrap() >= 1);
    assert_eq!(report["graph"]["shared"].as_array().unwrap().len(), 1);
}

#[test]
fn lucid_schedule_has_no_graph() {
    let (ok, text) = audit(&[
        "schedule",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert!(report["graph"].is_null());
    assert!(report["currents"].as_array().unwrap().is_empty());
}

#[test]
fn reads_admit_through_shadow_canary_and_active() {
    let (ok, text) = audit(&[
        "admit",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.admission/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["telemetry"]["schema"], "cyclone.telemetry/v0");
    assert_eq!(report["currents"].as_array().unwrap().len(), 1);
    assert_eq!(report["currents"][0]["lifecycle"], "active");
    assert_eq!(report["currents"][0]["compiled_routes"], 25);
    assert_eq!(report["currents"][0]["original_routes"], 5);
    assert_eq!(report["currents"][0]["rollbacks"], 0);
    let transitions = report["currents"][0]["transitions"].to_string();
    assert!(transitions.contains("shadow->canary"), "{transitions}");
    assert!(transitions.contains("canary->active"), "{transitions}");
    for decision in report["telemetry"]["decisions"].as_array().unwrap() {
        if decision["lifecycle"] == "shadow" {
            assert_ne!(decision["route"], "compiled");
        }
    }
}

#[test]
fn lucid_admit_has_no_compiled_routes() {
    let (ok, text) = audit(&[
        "admit",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["schema"], "cyclone.admission/v0");
    assert!(report["currents"].as_array().unwrap().is_empty());
    let compiled = report["telemetry"]["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|decision| decision["route"] == "compiled")
        .count();
    assert_eq!(compiled, 0);
}

#[test]
fn a_disabled_policy_suspends_the_read_journal() {
    let (ok, text) = audit(&[
        "admit",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        "--policy",
        trace("admit-disabled.policy.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["currents"][0]["lifecycle"], "suspended");
    assert_eq!(report["currents"][0]["compiled_routes"], 0);
}

#[test]
fn two_agents_reuse_one_read_compilation() {
    let (ok, text) = audit(&[
        "portfolio",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        "--economics",
        trace("portfolio-read.economics.json").to_str().unwrap(),
        trace("portfolio-agents.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.portfolio/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["leaked"], false);
    assert_eq!(report["benefit_minor"], 20);
    assert_eq!(
        report["patterns"][0]["agents"],
        serde_json::json!(["alpha", "beta"])
    );
    assert!(report["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|artifact| artifact["tenant_id"] == "tenant-a"));
}

#[test]
fn a_grant_adopts_the_read_for_the_consumer_tenant() {
    let (ok, text) = audit(&[
        "portfolio",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        "--economics",
        trace("portfolio-read.economics.json").to_str().unwrap(),
        "--grant",
        trace("portfolio-consumer.grant.json").to_str().unwrap(),
        trace("portfolio-agents.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["leaked"], false);
    assert_eq!(report["benefit_minor"], 40);
    let consumer = report["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["tenant_id"] == "tenant-b")
        .unwrap();
    assert!(consumer["observation_ids"].as_array().unwrap().is_empty());
    assert!(consumer["agent_ids"].as_array().unwrap().is_empty());
    let consumer_json = consumer.to_string();
    assert!(!consumer_json.contains("tenant-a"), "{consumer_json}");
    let neutral = report["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["neutral"] == true)
        .unwrap();
    assert!(!neutral.to_string().contains("tenant-a"));
}

#[test]
fn lucid_portfolio_has_no_pattern() {
    let (ok, text) = audit(&[
        "portfolio",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["schema"], "cyclone.portfolio/v0");
    assert!(report["patterns"].as_array().unwrap().is_empty());
    assert_eq!(report["benefit_minor"], 0);
}

#[test]
fn adversarial_corpus_contains_the_twelve_scenarios() {
    let (ok, text) = audit(&[
        "adversarial",
        "--json",
        trace("adversarial.json").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.adversarial/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["contained"], 12);
    assert_eq!(report["leaked"], false);
    assert_eq!(report["executable_effects"], 0);
    assert_eq!(report["scenarios"].as_array().unwrap().len(), 12);
    assert!(report.get("prompt").is_none());
    assert!(report.get("secret").is_none());
}

#[test]
fn adversarial_rejects_an_evidence_file() {
    let (ok, _text) = audit(&[
        "adversarial",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("adversarial.json").to_str().unwrap(),
    ]);
    assert!(!ok);
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

#[test]
fn the_read_journal_projects_an_active_dashboard() {
    let (ok, text) = audit(&[
        "dashboard",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.dashboard/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["record_scope"], "journal");
    assert_eq!(report["registry"].as_array().unwrap().len(), 1);
    assert_eq!(report["registry"][0]["pressure"], "high");
    assert_eq!(report["registry"][0]["status"], "active");
    assert_eq!(report["registry"][0]["scope"], "journal");
    assert_eq!(report["registry"][0]["owner"], "tenant-a");
    assert_eq!(report["registry"][0]["version"], 1);
    assert_eq!(report["registry"][0]["evidence"]["economics_minor"], 1);
    assert_eq!(report["graph"]["edges"].as_array().unwrap().len(), 0);
    assert_eq!(
        report["graph"]["critical_path"].as_array().unwrap().len(),
        1
    );
    assert_eq!(report["economics"]["avoided_inference"], "unmeasured");
    assert!(report["economics"]["value_minor"].is_null());
    assert_eq!(report["economics"]["benefit_minor"], 0);
    assert_eq!(report["safety"]["leaked"], false);
    assert_eq!(
        report["execution"]["fallbacks"].as_array().unwrap().len(),
        5
    );
    let hash = report["compilation"][0]["artifact_sha256"]
        .as_str()
        .unwrap();
    assert!(hash.starts_with("sha256:"), "{hash}");
    assert_eq!(hash.len(), 7 + 64);
    assert!(hash[7..].chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(
        report["compilation"][0]["transformations"],
        serde_json::json!(["interpret"])
    );
    assert_eq!(report["pressure_history"][0]["agreement"], "30/30");
    assert_secret_free(&report);

    let (again_ok, again) = audit(&[
        "dashboard",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(again_ok);
    assert_eq!(text, again);
}

#[test]
fn a_refused_environment_stays_on_the_safety_view() {
    let (ok, text) = audit(&[
        "dashboard",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("environment-split.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["registry"][0]["status"], "refused");
    assert_eq!(report["registry"][0]["scope"], "none");
    assert!(!report["safety"]["rejected_promotions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(report["graph"]["nodes"].as_array().unwrap().is_empty());
    assert!(report["graph"]["edges"].as_array().unwrap().is_empty());
    assert!(report["graph"]["critical_path"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn the_lucid_journal_has_an_empty_registry() {
    let (ok, text) = audit(&[
        "dashboard",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert!(report["registry"].as_array().unwrap().is_empty());
    assert_eq!(report["execution"]["actions"].as_array().unwrap().len(), 7);
    assert_secret_free(&report);
}

#[test]
fn a_disabled_policy_suspends_the_dashboard_current() {
    let (ok, text) = audit(&[
        "dashboard",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        "--policy",
        trace("admit-disabled.policy.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["registry"][0]["status"], "suspended");
    assert_eq!(report["registry"][0]["scope"], "journal");
    assert_eq!(
        report["pressure_history"][0]["drift_kinds"],
        serde_json::json!(["policy"])
    );
    assert!(!report["safety"]["suspended"].as_array().unwrap().is_empty());
}

#[test]
fn portfolio_economics_reach_the_dashboard() {
    let (ok, text) = audit(&[
        "dashboard",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        "--economics",
        trace("portfolio-read.economics.json").to_str().unwrap(),
        trace("portfolio-agents.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["economics"]["value_minor"], 240);
    assert_eq!(report["economics"]["break_even"], 4);
    assert_eq!(report["economics"]["benefit_minor"], 20);
    assert_eq!(report["economics"]["avoided_inference"], "unmeasured");
    assert_eq!(report["safety"]["leaked"], false);
}

#[test]
fn a_grant_adds_the_consumer_benefit_on_the_dashboard() {
    let (ok, text) = audit(&[
        "dashboard",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        "--economics",
        trace("portfolio-read.economics.json").to_str().unwrap(),
        "--grant",
        trace("portfolio-consumer.grant.json").to_str().unwrap(),
        trace("portfolio-agents.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["economics"]["benefit_minor"], 40);
    assert_eq!(report["safety"]["leaked"], false);
}

#[test]
fn html_is_rejected_outside_the_dashboard() {
    let (ok, _text) = audit(&[
        "audit",
        "--html",
        "dashboard.html",
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!ok);
}

#[test]
fn the_dashboard_page_names_the_seven_views() {
    let page = std::env::temp_dir().join(format!("cyclone-dashboard-{}.html", std::process::id()));
    let (ok, _text) = audit(&[
        "dashboard",
        "--html",
        page.to_str().unwrap(),
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok);
    let html = std::fs::read_to_string(&page).unwrap();
    let _ = std::fs::remove_file(&page);
    for title in [
        "Current registry",
        "Cyclone graph",
        "Pressure history",
        "Compilation report",
        "Economic report",
        "Execution trace",
        "Safety report",
    ] {
        assert!(html.contains(&format!("<h2>{title}</h2>")), "{title}");
    }
    assert!(html.contains("Deployment scope is the journal."));
}

fn assert_no_secret_keys(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                assert_ne!(key, "prompt");
                assert_ne!(key, "secret");
                assert_no_secret_keys(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                assert_no_secret_keys(item);
            }
        }
        _ => {}
    }
}

#[test]
fn the_read_journal_declares_an_environment_snapshot() {
    let evidence = trace("reads-30.evidence.json");
    let path = trace("reads-30.jsonl");
    let args = [
        "equivalence",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ];
    let (ok, text) = audit(&args);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.equivalence/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["contracts"].as_array().unwrap().len(), 1);
    assert_eq!(report["contracts"][0]["current_id"], "tenant-a:read");
    assert_eq!(report["contracts"][0]["category"], "read_only");
    assert_eq!(report["contracts"][0]["check"], "environment_snapshot");
    assert_eq!(report["contracts"][0]["tolerance"], "none");
    assert_eq!(report["contracts"][0]["shadow_pass"], true);
    assert_eq!(report["contracts"][0]["shadow_checked"], 30);
    assert_eq!(report["contracts"][0]["shadow_matched"], 30);
    assert_eq!(report["contracts"][0]["irreversible_replay"], false);
    assert!(report["undeclared"].as_array().unwrap().is_empty());
    assert_no_secret_keys(&report);
    let (again_ok, again) = audit(&args);
    assert!(again_ok);
    assert_eq!(text, again);
}

#[test]
fn the_pure_journal_declares_exact_output() {
    let (ok, text) = audit(&[
        "equivalence",
        "--json",
        "--evidence",
        trace("pure-30.evidence.json").to_str().unwrap(),
        trace("pure-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["contracts"][0]["category"], "pure");
    assert_eq!(report["contracts"][0]["check"], "exact_output");
    assert_eq!(report["contracts"][0]["tolerance"], "none");
    assert_eq!(report["contracts"][0]["irreversible_replay"], false);
}

#[test]
fn a_refused_environment_stays_undeclared() {
    let (ok, text) = audit(&[
        "equivalence",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("environment-split.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert!(report["contracts"].as_array().unwrap().is_empty());
    assert_eq!(report["undeclared"][0]["reason"], "promotion refused");
}

#[test]
fn the_lucid_journal_declares_nothing() {
    let (ok, text) = audit(&[
        "equivalence",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert!(report["contracts"].as_array().unwrap().is_empty());
    assert!(report["undeclared"].as_array().unwrap().is_empty());
}

#[test]
fn the_read_journal_records_a_one_attempt_policy() {
    let evidence = trace("reads-30.evidence.json");
    let path = trace("reads-30.jsonl");
    let args = [
        "guarantees",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ];
    let (ok, text) = audit(&args);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.guarantees/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["exactly_once"], false);
    assert_eq!(report["transactional"], false);
    assert_eq!(report["guarantees"].as_array().unwrap().len(), 1);
    assert_eq!(report["guarantees"][0]["current_id"], "tenant-a:read");
    assert_eq!(report["guarantees"][0]["category"], "read_only");
    assert_eq!(report["guarantees"][0]["kind"], "policy_retry");
    assert_eq!(report["guarantees"][0]["attempts_limit"], 1);
    assert_eq!(report["guarantees"][0]["attempts_performed"], 1);
    assert_eq!(report["guarantees"][0]["exactly_once"], false);
    assert_eq!(report["guarantees"][0]["operation_id"], "tenant-a:read");
    assert_eq!(report["guarantees"][0]["journal"], "memory");
    assert_eq!(report["guarantees"][0]["reconciliation"], false);
    assert_eq!(report["guarantees"][0]["transactional"], false);
    assert_eq!(report["guarantees"][0]["shadow_pass"], true);
    assert!(report["unclaimed"].as_array().unwrap().is_empty());
    assert_no_secret_keys(&report);
    let (again_ok, again) = audit(&args);
    assert!(again_ok);
    assert_eq!(text, again);
}

#[test]
fn the_pure_journal_records_a_three_attempt_limit() {
    let (ok, text) = audit(&[
        "guarantees",
        "--json",
        "--evidence",
        trace("pure-30.evidence.json").to_str().unwrap(),
        trace("pure-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["exactly_once"], false);
    assert_eq!(report["guarantees"][0]["category"], "pure");
    assert_eq!(report["guarantees"][0]["kind"], "retry_within_limit");
    assert_eq!(report["guarantees"][0]["attempts_limit"], 3);
    assert_eq!(report["guarantees"][0]["attempts_performed"], 1);
    assert_eq!(report["guarantees"][0]["exactly_once"], false);
    assert_eq!(report["guarantees"][0]["journal"], "memory");
}

#[test]
fn a_refused_environment_stays_unclaimed() {
    let (ok, text) = audit(&[
        "guarantees",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("environment-split.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["exactly_once"], false);
    assert!(report["guarantees"].as_array().unwrap().is_empty());
    assert_eq!(report["unclaimed"][0]["reason"], "promotion refused");
    assert_eq!(report["unclaimed"][0]["exactly_once"], false);
}

#[test]
fn the_lucid_journal_records_no_guarantee() {
    let (ok, text) = audit(&[
        "guarantees",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["exactly_once"], false);
    assert!(report["guarantees"].as_array().unwrap().is_empty());
    assert!(report["unclaimed"].as_array().unwrap().is_empty());
}

#[test]
fn guarantees_reject_dashboard_flags() {
    let (html_ok, _) = audit(&[
        "guarantees",
        "--html",
        "page.html",
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!html_ok);
    let (policy_ok, _) = audit(&[
        "guarantees",
        "--policy",
        trace("admit-disabled.policy.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!policy_ok);
}

#[test]
fn the_read_journal_records_the_observed_principal() {
    let evidence = trace("reads-30.evidence.json");
    let path = trace("reads-30.jsonl");
    let args = [
        "authority",
        "--json",
        "--evidence",
        evidence.to_str().unwrap(),
        path.to_str().unwrap(),
    ];
    let (ok, text) = audit(&args);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "cyclone.authority/v0");
    assert_eq!(report["pass"], true);
    assert_eq!(report["elevated"], false);
    assert_eq!(report["historical_permission"], false);
    assert_eq!(report["contexts"].as_array().unwrap().len(), 1);
    assert_eq!(report["contexts"][0]["current_id"], "tenant-a:read");
    assert_eq!(report["contexts"][0]["binding"], "owner");
    assert_eq!(report["contexts"][0]["tenant_id"], "tenant-a");
    assert_eq!(
        report["contexts"][0]["principals"],
        serde_json::json!(["preflight"])
    );
    assert_eq!(
        report["contexts"][0]["resources"],
        serde_json::json!([
            "sha256:3333333333333333333333333333333333333333333333333333333333333333"
        ])
    );
    assert_eq!(
        report["contexts"][0]["operations"],
        serde_json::json!(["read"])
    );
    assert_eq!(report["contexts"][0]["elevated"], false);
    assert_eq!(report["contexts"][0]["historical_permission"], false);
    assert!(report["unauthorised"].as_array().unwrap().is_empty());
    assert_no_secret_keys(&report);
    let (again_ok, again) = audit(&args);
    assert!(again_ok);
    assert_eq!(text, again);
}

#[test]
fn the_pure_journal_records_its_agent_principal() {
    let (ok, text) = audit(&[
        "authority",
        "--json",
        "--evidence",
        trace("pure-30.evidence.json").to_str().unwrap(),
        trace("pure-30.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["historical_permission"], false);
    assert_eq!(report["contexts"][0]["current_id"], "tenant-a:pure");
    assert_eq!(
        report["contexts"][0]["principals"],
        serde_json::json!(["agent"])
    );
    assert_eq!(
        report["contexts"][0]["operations"],
        serde_json::json!(["pure"])
    );
}

#[test]
fn a_refused_environment_stays_unauthorised() {
    let (ok, text) = audit(&[
        "authority",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        trace("environment-split.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["historical_permission"], false);
    assert!(report["contexts"].as_array().unwrap().is_empty());
    assert_eq!(report["unauthorised"][0]["reason"], "promotion refused");
    assert_eq!(report["unauthorised"][0]["elevated"], false);
}

#[test]
fn the_lucid_journal_records_no_authority() {
    let (ok, text) = audit(&[
        "authority",
        "--json",
        trace("lucid-session-v1.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["historical_permission"], false);
    assert!(report["contexts"].as_array().unwrap().is_empty());
    assert!(report["unauthorised"].as_array().unwrap().is_empty());
}

#[test]
fn a_grant_names_the_consumer_principal_on_the_authority_record() {
    let (ok, text) = audit(&[
        "authority",
        "--json",
        "--evidence",
        trace("reads-30.evidence.json").to_str().unwrap(),
        "--grant",
        trace("portfolio-consumer.grant.json").to_str().unwrap(),
        trace("portfolio-agents.jsonl").to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["elevated"], false);
    assert_eq!(report["contexts"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["contexts"][0]["principals"],
        serde_json::json!(["alpha", "beta"])
    );
    let consumer = &report["contexts"][1];
    assert_eq!(consumer["binding"], "consumer");
    assert_eq!(consumer["current_id"], "tenant-b:read");
    assert_eq!(consumer["tenant_id"], "tenant-b");
    assert_eq!(
        consumer["principals"],
        serde_json::json!(["portfolio-clerk"])
    );
    assert_eq!(consumer["operations"], serde_json::json!(["read"]));
    assert_eq!(consumer["historical_permission"], false);
    let encoded = consumer.to_string();
    assert!(!encoded.contains("tenant-a"), "{encoded}");
    assert!(!encoded.contains("alpha"), "{encoded}");
    assert!(!encoded.contains("beta"), "{encoded}");
}

#[test]
fn authority_rejects_dashboard_flags() {
    let (html_ok, _) = audit(&[
        "authority",
        "--html",
        "page.html",
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!html_ok);
    let (policy_ok, _) = audit(&[
        "authority",
        "--policy",
        trace("admit-disabled.policy.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!policy_ok);
    let (economics_ok, _) = audit(&[
        "authority",
        "--economics",
        trace("portfolio-read.economics.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!economics_ok);
}

#[test]
fn equivalence_rejects_dashboard_flags() {
    let (html_ok, _) = audit(&[
        "equivalence",
        "--html",
        "page.html",
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!html_ok);
    let (policy_ok, _) = audit(&[
        "equivalence",
        "--policy",
        trace("admit-disabled.policy.json").to_str().unwrap(),
        trace("reads-30.jsonl").to_str().unwrap(),
    ]);
    assert!(!policy_ok);
}
