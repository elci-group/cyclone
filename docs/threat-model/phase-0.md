# Threat model, Phase 0

- Status: baseline for review
- Date: 2026-11-08
- Directive: CTD-001 §7, §9, §14
- Scope: Repository Preflight and the Lucid and Tov interfaces it will sit on

Cyclone has no runtime in this checkout. The controls below are obligations on Phase 1, matched to behaviour that already exists in neighbouring tools. Nothing here is a deployed security boundary.

## Assets

- Tenant-scoped traces and the current registry, once they exist.
- Compilation artifacts and the evidence that promoted them.
- Authority to run tools: Lucid `capability`, `lease_epoch`, and `snapshot_id`.
- Repository contents read by preflight. Manifests are hashed. Bodies are not stored.
- Economic claims. A false saving is an integrity failure.

## Trust boundaries

| Boundary | What is trusted | What is not |
| --- | --- | --- |
| Model output | Nothing | Lucid rejects unknown action schemas and ignores tenant ids supplied by the model |
| Redacted session journal | Tool name, status, reject reason, token counts | Paths, prompts, secrets, and therefore objective |
| Manifest digest | Bytes of `Cargo.toml` and `package.json` at lock time | Path-dependency targets, dirty patches, `target/` |
| `git -C` | A directory that contains its own `.git` | A child of `/home/sal`. The walk reaches the parent repo. `/home/sal/toungsten` is empty and still reports a work tree |
| Compiler diagnostics | `tov check --json` as a read of one tree | A `safe` fix as permission to edit |

## Adversarial scenarios

The twelve scenarios in CTD-001 §14, as they already show up on this machine.

1. **Hidden environment.** A path dependency can point outside the repository. Lastdown depends on absolute `/home/sal/3form`. Lucid depends on absolute `/home/sal/:e/crates/colony-core`, `/home/sal/:e/crates/colony-planner`, and `/home/sal/mesut/crates/mesut`. Bank and Padagonia depend on sibling paths `../3form` and `../bound/crates/bound-core`. The same preflight steps succeed while those neighbours differ. `environment_digest` has to cover those targets. The corpus lock does not.
2. **Non-idempotent work.** Repository Preflight does not mutate. `write_worktree`, `propose_patch`, and `apply_source` are outside it. Lucid receipts already treat a reused command id with different bytes as `idempotency_conflict`. Cyclone must not re-issue an external effect to prove equivalence.
3. **Prompt injection into a current.** The session journal drops the prompt before persistence. Repetition of a redacted tool name is a weak signal. Promotion requires the pre-redaction argument digest, a capability check, and a shadow comparison. Repetition alone stays evidence.
4. **Concurrent parent events.** Lucid orders by `sequence` inside one `execution_id`. There is no `parent_span_id`. Phase 1 must refuse to invent a parent edge from `occurred_at`.
5. **Dependency drift.** The corpus lock fails when HEAD or a manifest sha256 changes. It does not hash `form3`, `colony-core`, `mesut`, or `bound-core`. A current compiled against one of those trees goes stale silently until that gap is closed.
6. **Cross-tenant structural twins.** Lucid stamps `tenant_id` on events, commands, snapshots, and evidence. Cyclone has no registry. Identical preflight shapes across `/home/sal/bank` and `/home/sal/lastdown` stay separate tenants. Cross-tenant reuse needs a fresh authorisation under the consuming tenant.
7. **Partial cyclone failure.** No graph runs yet. A failed `git` call is recorded as a null identity field. Phase 1 has to define cancellation before any child current exists.
8. **Stale cache.** Second samples on this host were often faster than first samples. That is page cache, not semantic equivalence. Preflight results are not reused as a current.
9. **Cancellation races.** No scheduler exists. The obligation is recorded so Phase 3 does not treat thread sharing as synchronisation. CTD-001 already separates sync contracts from threads.
10. **Rare branches.** Tov lists eight tests as explicit `[[test]]` entries with `harness = false`. The other five crates rely on Cargo's `tests/*.rs` discovery. A miner that only saw autodiscovery would miss the Tov layout. The procedure records both.
11. **False economy.** Median mechanical cost is 14.921 ms. Monitoring that exceeds the saved work wipes `V_c`. Inference, which might dominate, is unmeasured. ADR 0004 blocks economic promotion.
12. **Same sequence, different objective.** After redaction, `tool.completed` keeps `tool`, `status`, and `reject_reason`. Two `read` actions match. The adapter hashes arguments before redaction and stores the digest.

## Controls already present next door

- Lucid fail-closed parsing, path escape, lease epoch, capability denial, idempotency conflict, config mode `0600`, and redaction. Documented in `/home/sal/lucid-cli/docs/security.md` and enforced in `lucid-contract`.
- Deliver 1.0.1 as a local, non-network gate. This checkout uses it to bind the corpus.
- Padagonia's separation of observation from assertion. Useful later as provenance vocabulary. Not linked.

## Controls this checkout does not provide

A tenant-isolated registry, signed artifacts, quotas, revocation, trace-poisoning defences, and an emergency disable switch are Phase 1 and Phase 4 work. Phase 0's substitute is narrow: the preflight process is read-only, it does not read environment variables into the report, it does not store manifest bodies, and it does not call a model.

## Acceptance stance

CTD-001 §13 tolerates zero privilege escalation and zero undeclared external side effects. This phase meets that stance by performing neither execution nor promotion. It is not a test of a runtime.
