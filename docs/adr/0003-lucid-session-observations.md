# ADR 0003: Lucid sessions are the observation source

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §3.1, §3.2, §9

## Context

CTD-001 requires causal ordering that does not depend on wall-clock timestamps, tenant isolation, redaction, and a hard boundary around model calls. Lucid already records agent work as versioned JSON documents.

## Decision

Phase 1 ingestion will map `lucid.session/v1` journals into `ActionObservation` values. Cyclone will not invent a second causal clock.

Ordering key: `(tenant_id, execution_id, sequence)`. `occurred_at` is a display timestamp. `trace_id` on `lucid.event/v1` is optional and is copied when present. It does not replace `sequence`.

Duplicate `event_id` values collapse to one observation. The vendored fixture `contracts/v1/fixtures/session-v1.jsonl` repeats `evt_4` at sequence 4. That line is one observation.

Compiled currents target `lucid.command/v1`, which carries `capability`, `snapshot_id`, `lease_epoch`, and `operation`. They do not bypass that envelope by replaying a `lucid.action/v1` document. Historical observation of a privileged action is not permission to run it again. Lucid already rejects `capability_denied`, `stale_epoch`, `path_escape`, and `idempotency_conflict`.

`model.response` is generative. It is stored as evidence of cost and is ineligible for compilation. `message.applied` is orchestration. `task.running` and `task.completed` are trace boundaries.

## Field map

| ActionObservation | Lucid source today | Gap |
| --- | --- | --- |
| `observation_id` | `event_id` | Dedup required |
| `trace_id` | `execution_id`, plus optional event `trace_id` | Session lines have no `trace_id` |
| `parent_span_id` | Absent | Do not infer a parent from `occurred_at` |
| `agent_id` | `actor_id` or `agent.role` | Provider and model stay attributes, not authority |
| `tenant_id` | `tenant_id` | Assigned by Lucid, never by model output |
| `action_kind` | `detail.tool`, else event `kind` | Journal redaction keeps the tool name |
| `input_digest` | Absent on the redacted line | Hash arguments before redaction. Store the digest |
| `output_digest` | Absent | Same rule for tool results |
| `environment_digest` | `lucid.snapshot/v1` `content_sha256` when a command names a `snapshot_id` | Session lines do not carry it |
| `started_at`, `completed_at` | Single `occurred_at` | Duration is unknown from the journal alone |
| `effect_class` | Derived from kind | `run_checks` stays unknown until the check command is classified |
| `outcome` | `detail.status`, `detail.reject_reason`, receipt `status` | |
| `evidence_refs` | `payload_sha256` hashes the redacted detail | Not an equivalence proof |

`redact_detail` in `lucid-contract` keeps, for `tool.completed`, only `tool`, `status`, and `reject_reason`. Prompts, paths, and secrets are stripped, per `docs/security.md`. Two reads of different files become indistinguishable after redaction. Scenario 12 in the directive follows directly: matching action sequences can hide different objectives. An observation adapter hashes the authorized arguments first, then lets Lucid drop the bytes.

Effect classes for the agent vocabulary:

| Kind | Class for compilation |
| --- | --- |
| `read` | Read-only, once the path is bound to a snapshot |
| `run_checks` | Unknown. Checks may write build outputs |
| `write_worktree`, `propose_patch`, `apply_source` | Filesystem mutation. Outside Repository Preflight |
| `finish` | Control event |
| `model.response` | Generative. Never compiled |

Runner operations `provision`, `pause`, `stop`, `reconcile`, and `analyze` stay outside the reference workload.

Evidence documents (`lucid.evidence/v1`) accept producers `uni`, `dreamseq`, `deliver`, and `runner`. Cyclone will ask for its own producer id when it emits that schema. Until the contract accepts one, Cyclone evidence stays in this repository.

## Evidence

- Schemas read from `/home/sal/lucid-cli/contracts/v1/`: `event`, `command`, `receipt`, `evidence`, `snapshot`.
- Session construction and redaction read from `crates/lucid-contract/src/lib.rs`.
- Action kinds: `read`, `write_worktree`, `run_checks`, `propose_patch`, `finish`.
- Command operations: `provision`, `read`, `write_worktree`, `run_checks`, `pause`, `stop`, `reconcile`, `apply_source`, `analyze`.
- Lucid HEAD `e10024bfecf3d4ee6846aced5282a9ec55f148fc` on `main`. No `origin` remote is configured. Twelve tracked paths were dirty at measurement time. The lock records the count and binds manifest digests, not the patch.

## Consequences

Phase 1 can replay a versioned session fixture into observations and show a stable sequence. It cannot claim input equivalence from a redacted journal. Shadow mode for mutating commands uses the receipt's idempotency rule and refuses a second external effect.
