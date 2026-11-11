//! `cyclone` audits a trace, compiles eligible currents, shadow-checks them, schedules them, admits them, portfolios them, contains hostile traces, projects an operator dashboard, declares an equivalence contract, records an execution guarantee, or records an authority context.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use cyclone::{
    render_admit, render_adversarial, render_authority, render_compile, render_dashboard,
    render_dashboard_html, render_equivalence, render_guarantees, render_portfolio,
    render_schedule, render_text, render_verify,
};
use cyclone_pressure::EvidenceTable;
use cyclone_registry::{Economics, Grant};
use cyclone_runtime::Policy;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("cyclone: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args = parse_args()?;
    let text = fs::read_to_string(&args.file)
        .map_err(|error| format!("cannot read {}: {error}", args.file.display()))?;
    let evidence = match &args.evidence {
        Some(path) => {
            let body = fs::read_to_string(path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            EvidenceTable::parse(&body).map_err(|error| error.to_string())?
        }
        None => EvidenceTable::empty(),
    };
    match args.command {
        Command::Audit => {
            let report = cyclone::audit(&text, &evidence).map_err(|error| error.to_string())?;
            emit(args.json, &report, render_text(&report))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Compile => {
            let report =
                cyclone::compile_trace(&text, &evidence).map_err(|error| error.to_string())?;
            emit(args.json, &report, render_compile(&report))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Verify => {
            let report =
                cyclone::verify_trace(&text, &evidence).map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_verify(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Schedule => {
            let report =
                cyclone::schedule_trace(&text, &evidence).map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_schedule(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Admit => {
            let policy = match &args.policy {
                Some(path) => {
                    let body = fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    Policy::parse(&body).map_err(|error| error.to_string())?
                }
                None => Policy::standard(),
            };
            let report = cyclone::admit_trace(&text, &evidence, &policy)
                .map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_admit(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Portfolio => {
            let economics = match &args.economics {
                Some(path) => {
                    let body = fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    Economics::parse(&body).map_err(|error| error.to_string())?
                }
                None => Economics::empty(),
            };
            let grant = match &args.grant {
                Some(path) => {
                    let body = fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    Some(Grant::parse(&body).map_err(|error| error.to_string())?)
                }
                None => None,
            };
            let report = cyclone::portfolio_trace(&text, &evidence, &economics, grant.as_ref())
                .map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_portfolio(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Adversarial => {
            let report = cyclone::adversarial_trace(&text).map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_adversarial(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Dashboard => {
            let policy = match &args.policy {
                Some(path) => {
                    let body = fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    Policy::parse(&body).map_err(|error| error.to_string())?
                }
                None => Policy::standard(),
            };
            let economics = match &args.economics {
                Some(path) => {
                    let body = fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    Economics::parse(&body).map_err(|error| error.to_string())?
                }
                None => Economics::empty(),
            };
            let grant = match &args.grant {
                Some(path) => {
                    let body = fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    Some(Grant::parse(&body).map_err(|error| error.to_string())?)
                }
                None => None,
            };
            let report =
                cyclone::dashboard_trace(&text, &evidence, &policy, &economics, grant.as_ref())
                    .map_err(|error| error.to_string())?;
            if let Some(path) = &args.html {
                fs::write(path, render_dashboard_html(&report))
                    .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            }
            let pass = report.pass;
            emit(args.json, &report, render_dashboard(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Equivalence => {
            let report =
                cyclone::equivalence_trace(&text, &evidence).map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_equivalence(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Guarantees => {
            let report =
                cyclone::guarantees_trace(&text, &evidence).map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_guarantees(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Authority => {
            let grant = match &args.grant {
                Some(path) => {
                    let body = fs::read_to_string(path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    Some(Grant::parse(&body).map_err(|error| error.to_string())?)
                }
                None => None,
            };
            let report = cyclone::authority_trace(&text, &evidence, grant.as_ref())
                .map_err(|error| error.to_string())?;
            let pass = report.pass;
            emit(args.json, &report, render_authority(&report))?;
            if pass {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Reliance => {
            let report = cyclone::reliance_trace(&text).map_err(|error| error.to_string())?;
            let rendered = format!(
                "schema: {}\nwindow: previous {} turns\nmethod: {}\n{}",
                report.schema,
                cyclone::RELIANCE_WINDOW,
                report.method,
                report
                    .records
                    .iter()
                    .map(|r| format!(
                    "{} tenant={} execution={} reliance={:?} confidence={:?} nearest_predecessor={:?} status={}",
                    r.task_id, r.tenant_id, r.execution_id, r.reliance,
                    r.confidence, r.nearest_predecessor_turns_back, r.status
                ))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            emit(args.json, &report, rendered)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn emit(json: bool, report: &impl serde::Serialize, text: String) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).map_err(|error| error.to_string())?
        );
    } else {
        print!("{text}");
    }
    Ok(())
}

enum Command {
    Audit,
    Compile,
    Verify,
    Schedule,
    Admit,
    Portfolio,
    Adversarial,
    Dashboard,
    Equivalence,
    Guarantees,
    Authority,
    Reliance,
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Audit => "audit",
            Command::Compile => "compile",
            Command::Verify => "verify",
            Command::Schedule => "schedule",
            Command::Admit => "admit",
            Command::Portfolio => "portfolio",
            Command::Adversarial => "adversarial",
            Command::Dashboard => "dashboard",
            Command::Equivalence => "equivalence",
            Command::Guarantees => "guarantees",
            Command::Authority => "authority",
            Command::Reliance => "reliance",
        }
    }
}

struct Args {
    command: Command,
    json: bool,
    evidence: Option<PathBuf>,
    policy: Option<PathBuf>,
    economics: Option<PathBuf>,
    grant: Option<PathBuf>,
    html: Option<PathBuf>,
    file: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut raw = env::args().skip(1);
    let command = match raw.next().as_deref() {
        Some("audit") => Command::Audit,
        Some("compile") => Command::Compile,
        Some("verify") => Command::Verify,
        Some("schedule") => Command::Schedule,
        Some("admit") => Command::Admit,
        Some("portfolio") => Command::Portfolio,
        Some("adversarial") => Command::Adversarial,
        Some("dashboard") => Command::Dashboard,
        Some("equivalence") => Command::Equivalence,
        Some("guarantees") => Command::Guarantees,
        Some("authority") => Command::Authority,
        Some("reliance") => Command::Reliance,
        _ => return Err(usage()),
    };
    let name = command.name();
    let mut json = false;
    let mut evidence = None;
    let mut policy = None;
    let mut economics = None;
    let mut grant = None;
    let mut html = None;
    let mut file = None;
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--evidence" => {
                let path = raw
                    .next()
                    .ok_or_else(|| "--evidence needs a path".to_string())?;
                evidence = Some(PathBuf::from(path));
            }
            "--policy" => {
                let path = raw
                    .next()
                    .ok_or_else(|| "--policy needs a path".to_string())?;
                policy = Some(PathBuf::from(path));
            }
            "--economics" => {
                let path = raw
                    .next()
                    .ok_or_else(|| "--economics needs a path".to_string())?;
                economics = Some(PathBuf::from(path));
            }
            "--grant" => {
                let path = raw
                    .next()
                    .ok_or_else(|| "--grant needs a path".to_string())?;
                grant = Some(PathBuf::from(path));
            }
            "--html" => {
                let path = raw
                    .next()
                    .ok_or_else(|| "--html needs a path".to_string())?;
                html = Some(PathBuf::from(path));
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown argument {other}"));
            }
            other => {
                if file.is_some() {
                    return Err(format!("{name} accepts one trace file"));
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or_else(|| format!("{name} needs a trace file"))?;
    if policy.is_some() && !matches!(command, Command::Admit | Command::Dashboard) {
        return Err("--policy applies to admit and dashboard".to_string());
    }
    if economics.is_some() && !matches!(command, Command::Portfolio | Command::Dashboard) {
        return Err("--economics applies to portfolio and dashboard".to_string());
    }
    if grant.is_some()
        && !matches!(
            command,
            Command::Portfolio | Command::Dashboard | Command::Authority
        )
    {
        return Err("--grant applies to portfolio, dashboard, and authority".to_string());
    }
    if evidence.is_some() && matches!(command, Command::Adversarial) {
        return Err("--evidence does not apply to adversarial".to_string());
    }
    if html.is_some() && !matches!(command, Command::Dashboard) {
        return Err("--html applies to dashboard".to_string());
    }
    Ok(Args {
        command,
        json,
        evidence,
        policy,
        economics,
        grant,
        html,
        file,
    })
}

fn usage() -> String {
    "usage: cyclone audit|compile|verify|schedule|admit|portfolio|adversarial|dashboard|equivalence|guarantees|authority|reliance [--json] [--html FILE] [--evidence FILE] [--policy FILE] [--economics FILE] [--grant FILE] TRACE"
        .to_string()
}
