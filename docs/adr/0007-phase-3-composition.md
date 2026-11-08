# ADR 0007: Phase 3 schedules compiled currents as a typed graph

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §4, §5, §12 Phase 3
- Narrows: the scheduling sentence reserved for `cyclone-runtime` in ADR 0006

## Context

Phase 2 emits one `cyclone.cir/v0` value per eligible pure or read-only signature and checks it alone. CTD-001 Phase 3 exits when composed currents preserve declared causal order and side-effect semantics. The production lifecycle in Phase 4 stays ahead of this step.

## Decision

`cyclone-compose` builds `cyclone.graph/v0` from compiled currents and the traces that produced them.

| Piece | Rule |
| --- | --- |
| Nodes | One node per compiled current. |
| Edges | Consecutive successful segments in one execution. |
| Data-synchronous | A child input digest equals a parent output digest. |
| Phase-synchronous | That binding addresses an earlier step of a multi-step parent. |
| Event-synchronous | The boundary is `model.response`. |
| Sequential | The boundary is a completion control event. |
| Waves | Currents with no remaining parents. A wave is the concurrency set. |
| Critical path | One longest path. Equal lengths resolve to the lexicographically earlier path. |
| Shared computation | Pure currents with the same environment, input, and output evaluate once. Read-only currents share under that same rule. |
| Repeat | The current's recorded retry bound, 1 or 3. |
| Guards | A child waits for every parent. Cancellation propagates. Failure stops the dependent chain. |

`cyclone schedule` writes `cyclone.schedule/v0` and runs the graph as digest lookups. The process exits 1 when that run breaks an edge, a binding, or an output. A trace that compiles nothing, and a trace whose candidates are refused, both exit 0 with no graph. An unbounded cycle is refused before any lookup.

Low-pressure children record `inherit_context` when the parent environment digest matches. The lookup still uses the child's own digest.

`cyclone-runtime`, `cyclone-registry`, `cyclone-telemetry`, and `cyclone-tov` stay reserved. Live tool calls, mutation, native codegen, canary routing, and drift detection stay outside this phase. Repository Preflight stays a mechanical baseline.

## Consequences

`traces/compose-chain.jsonl` schedules `pure`, then `read`, then `normalise`. The first edge is data-synchronous. The second is sequential. The effect log is `pure`, `read_only`, `pure`. `traces/compose-share.jsonl` evaluates the shared pure lookup once. A reversed pair of edges is an unbounded cycle and produces no run.
