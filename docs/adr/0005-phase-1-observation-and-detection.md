# ADR 0005: Phase 1 classifies traces and does not deploy them

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §3.1, §3.2, §3.3, §12 Phase 1
- Supersedes: the scheduling sentence in ADR 0001 that held every crate until a separate policy approval

## Context

Phase 0 found a Lucid session journal with sequence numbers, a Tov compiler CLI, and a mechanical preflight whose agent-observation count is zero. CTD-001 Phase 1 exits when candidates can be reproduced from versioned traces and manually audited.

## Decision

The workspace builds four crates, edition 2021:

| Crate | Responsibility |
| --- | --- |
| `cyclone-observe` | Ingest `lucid.session/v1` and `cyclone.observation/v0`. Order by `(tenant_id, execution_id, sequence)`. Collapse identical observation ids. Reject conflicting ones. |
| `cyclone-detect` | Cut segments on model calls, control events, and unsuccessful actions. Mine exact action signatures. |
| `cyclone-pressure` | Apply the provisional gates from CTD-001 §3.3 and explain each one. |
| `cyclone` | `cyclone audit` writes the text or JSON report. |

`cyclone-ir`, `cyclone-compile`, `cyclone-runtime`, `cyclone-verify`, `cyclone-registry`, `cyclone-telemetry`, and `cyclone-tov` stay reserved.

The provisional gates are the classifier policy. They are not production service levels. An unmeasured replay or an unmeasured `V_c` fails the gate. Mechanical observations increment `mechanical_executions` and do not increment the agent count. Eligibility is a classification result. This phase does not install a current, call a tool, or change a repository.

Structural agreement is `matching / considered`, where both numbers count distinct agent executions that produced a compilable segment. The comparison is integer: high pressure needs `matching * 100 >= considered * 99`.

## Consequences

`cyclone audit traces/lucid-session-v1.jsonl` reproduces the vendored Lucid fixture: one duplicate collapsed, seven observations, no candidate. `traces/reads-30.jsonl` is eligible only when `traces/reads-30.evidence.json` attaches a passed replay and a positive economic term. The same signature with two environment digests stays refused.
