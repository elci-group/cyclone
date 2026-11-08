# ADR 0008: Phase 4 admits a journal under an independent lifecycle

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §6, §7.2, §8, §9, §12 Phase 4
- Narrows: the runtime and telemetry sentences reserved in ADR 0007

## Context

Phase 3 schedules compiled currents and checks causal order. CTD-001 Phase 4 exits when fault injection, isolation, recovery, and security acceptance tests pass. Lifecycle is independent of pressure: a high-pressure current can roll back, and a low-pressure current can become active. A gust stays observed.

## Decision

`cyclone-runtime` admits compiled currents against the journal that produced them. `cyclone-telemetry` records the decisions and the drift signals. The report schema is `cyclone.admission/v0`. The telemetry schema is `cyclone.telemetry/v0`. The policy schema is `cyclone.policy/v0`.

| Piece | Rule |
| --- | --- |
| Initial lifecycle | Shadow. An initial canary or active policy is refused. |
| Shadow | Compare the recorded output and serve `original_agent_workflow`. |
| Canary | Modulus at least 2. The first canary decision is one compiled lookup. The following modulus-minus-one decisions stay on the original workflow and still compare. |
| Active | Serve the compiled lookup until a fault. |
| Promotion | The hysteresis default is 3 consecutive clean decisions from shadow to canary, and from canary to active. One fault demotes to `rolled_back`. |
| Recovery | Three clean decisions return a rolled-back current to shadow. |
| Disable | `enabled: false` suspends every current and compiles nothing. |
| Invalid current | A privileged effect, economics at or below zero, or a failed type check suspends that current. The interpreter does not run. |
| Tenant | A tenant mismatch suspends that current. Other tenants keep their own counters. |
| Predecessor | A rolled-back step withholds later steps in the same execution. The child lifecycle stays where it was. |
| Record | Decisions store digests' identities and reasons. Prompts, paths, and secrets stay out. |

`cyclone admit` writes the report and exits 1 when a compiled route appears outside canary or active, or when a rollback or a withheld decision leaves the original workflow. A rollback that the runtime enforced exits 0.

The interpreter performs one lookup on a compiled route. Exactly-once external effects stay outside this phase. `cyclone-registry` and `cyclone-tov` stay reserved. Live tool calls, mutation, native codegen, and a network canary stay outside this phase. Repository Preflight stays a mechanical baseline. A passing admission is journal evidence for the lifecycle.

## Consequences

`traces/reads-30.jsonl` with its evidence table admits one high-pressure current. Under the default policy the journal ends active, with 25 compiled routes, 5 original routes, and 0 rollbacks. Transitions are `shadow->canary` and `canary->active`. `traces/admit-disabled.policy.json` suspends that same journal and compiles nothing. The Lucid fixture admits with an empty current list.
