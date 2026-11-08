# Phase 3

Cyclone composition. Exit condition from CTD-001: composed currents preserve declared causal order and side-effect semantics.

## Commands

`cyclone schedule` compiles the trace, builds `cyclone.graph/v0`, and runs that graph as digest lookups. The report schema is `cyclone.schedule/v0`.

```sh
cyclone schedule --json --evidence traces/compose-chain.evidence.json traces/compose-chain.jsonl
cyclone schedule --json --evidence traces/compose-share.evidence.json traces/compose-share.jsonl
cyclone schedule --json traces/lucid-session-v1.jsonl
```

The process exits 0 when `pass` is true. It exits 1 when a started current breaks its parents, its bindings, or its recorded output. A refusal to compose, including an unbounded cycle, is a completed report with no run.

## Synchronousity

| Trigger | When it is recorded |
| --- | --- |
| `data` | A child input digest is a parent output digest. |
| `phase` | That digest is an earlier step of a multi-step parent. |
| `event` | The segments are split by `model.response` and no digest binds them. |
| `sequential` | The segments are split by a completion control event. |

A child starts after every parent trigger is satisfied. A wave lists currents that have no remaining parents. The interpreter walks a wave in node-id order so the log is stable. The wave is the concurrency contract.

Pure currents whose environment, input, and output match share one evaluation. Read-only currents share on that same key. A pure current and a read-only current keep separate evaluations. The recorded retry bound is the repeat count: 3 for pure, 1 for read-only.

## Versioned traces

| File | What it shows |
| --- | --- |
| `traces/compose-chain.jsonl` | Thirty executions of `pure`, then `read`, then `normalise`. Data edge, then a sequential edge. The critical path length is 3. Effects `pure`, `read_only`, `pure`. |
| `traces/compose-chain.evidence.json` | Passed replay and economic term `1` for each of those signatures. |
| `traces/compose-share.jsonl` | `pure` and `normalise` carry the same pure lookup. The second current reuses the first evaluation. |
| `traces/compose-share.evidence.json` | Passed replay and economic term `1` for `pure` and `normalise`. |

Repository Preflight stays the mechanical Phase 0 baseline. A scheduled graph is equivalence evidence for order and effects. Phase 4, recorded in ADR 0008, admits that journal under an independent lifecycle.
