# Phase 4

Adaptive admission. Exit condition from CTD-001: fault injection, isolation, recovery, and security acceptance tests pass for the lifecycle.

## Commands

`cyclone audit` classifies a trace. `cyclone compile` writes `cyclone.cir/v0`. `cyclone verify` checks shadow equivalence. `cyclone schedule` runs the typed graph. `cyclone admit` replays the journal under `cyclone.policy/v0` and writes `cyclone.admission/v0`.

```sh
cyclone admit --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
cyclone admit --json --evidence traces/reads-30.evidence.json --policy traces/admit-disabled.policy.json traces/reads-30.jsonl
cyclone admit --json traces/lucid-session-v1.jsonl
```

The process exits 0 when `pass` is true. A rollback that the runtime enforced is a completed report. The process exits 1 when a compiled route appears outside canary or active, or when a rollback or withheld decision leaves the original workflow.

## Lifecycle

Pressure stays on the compiled current. Lifecycle moves on its own: `shadow`, `canary`, `active`, `suspended`, `rolled_back`, and `observed`.

The default policy starts at shadow, uses canary modulus 10, and promotes after 3 consecutive clean decisions. One fault demotes to `rolled_back` and serves the original workflow. Three clean decisions after that return the current to shadow. The first canary decision is a compiled lookup. The next modulus-minus-one canary decisions stay on the original workflow and still compare.

On `traces/reads-30.jsonl` with its evidence table, that journal records 30 clean reads. The first three decisions stay in shadow on the original workflow, then the current promotes. The canary window serves one compiled lookup and two original lookups, then the current promotes to active. The remaining 24 decisions are active compiled lookups. The report shows compiled_routes 25, original_routes 5, rollbacks 0, and lifecycle `active`. Transitions include `shadow->canary` and `canary->active`.

A disabled policy sets lifecycle `suspended` and compiled_routes 0. The Lucid fixture admits with pass true and no compiled routes.

## Telemetry

Each decision records the execution, the current, the tenant, the lifecycle at that decision, the pressure, the route, the outcome, and the reason. The decision carries no prompt, path, or secret. Drift kinds are environment, output, fallback, economics, side_effect, tenant, and policy. The telemetry schema is `cyclone.telemetry/v0`.

Repository Preflight stays the mechanical Phase 0 baseline. A passing admission is journal evidence for the lifecycle. It is a record of routes already taken in the journal. The interpreter performs one lookup on a compiled route. Phase 5, recorded in ADR 0009, reuses a registered current under a fresh grant. External effects stay outside this phase.
