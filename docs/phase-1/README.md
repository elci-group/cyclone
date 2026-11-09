# Phase 1

Observation, segmentation, and pressure classification. Exit condition from CTD-001: candidates reproduced from versioned traces, with an audit a person can read.

## Audit

```sh
cyclone audit traces/lucid-session-v1.jsonl
cyclone audit --json traces/reads-30.jsonl
cyclone audit --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
```

The text report lists every gate. `--json` writes `cyclone.audit/v0`.

Causal order is `(tenant_id, execution_id, sequence)`. Candidate mining groups by both tenant and action signature; execution counts, agreement, and determinism evidence never pool across tenants. Identical `(execution_id, observation_id)` lines collapse. A repeated id with different content is an error. Timestamps must be valid RFC 3339 calendar timestamps; Cyclone observation intervals also reject a completion earlier than their start, comparing offsets as instants. Timestamps do not sort the trace. `parent_span_id` stays empty on Lucid input, because that journal has no parent span.

`model.response` is generative. It ends a segment and is not part of the signature. Control events (`task.running`, `task.completed`, `message.applied`, `finish`) are boundaries. Rejected, failed, and denied actions are boundaries. A successful `run_checks` stays in the segment and fails the side-effect gate, because its effects are unknown.

## Gates

The classifier uses the provisional counts from CTD-001 §3.3.

| Gate | High pressure | Low pressure |
| --- | --- | --- |
| Observations | ≥30 agent executions | ≥15 agent executions |
| Structural agreement | ≥99/100 | ≥90/100 |
| Replay | passed | passed |
| Parameters | fixed input digests | fixed or bounded |
| Determinism | one output digest per input | one output digest per input |
| Economics | attached value > 0 | attached value > 0 |
| Side effects | no unknown class | no unknown class |
| Environment | one environment digest | one environment digest |

A missing digest, an unmeasured replay, or an unmeasured economic term fails the gate. The report names the failure. Eligibility is not deployment.

## Versioned traces

| File | What it shows |
| --- | --- |
| `traces/lucid-session-v1.jsonl` | Vendored Lucid fixture. Duplicate `evt_4`. No successful non-generative segment. |
| `traces/reads-30.jsonl` | Thirty agent reads, one digest. Gust until evidence is attached, then high. |
| `traces/reads-30.evidence.json` | Passed replay and economic term `1` for signature `read`. |
| `traces/environment-split.jsonl` | The same signature under two environment digests. Refused with the evidence file attached. |
| `traces/mechanical-preflight.jsonl` | Six mechanical reads. Agent executions stay 0. |

Evidence tables use the exact `cyclone.evidence-table/v0` fields. Unknown top-level or entry fields are rejected so misspelled or unsupported evidence cannot be silently ignored.

Repository Preflight's Phase 0 baseline is unchanged. Those samples are not agent observations, and this phase does not promote them. Phase 2, recorded in ADR 0006, compiles the eligible pure and read-only currents.
