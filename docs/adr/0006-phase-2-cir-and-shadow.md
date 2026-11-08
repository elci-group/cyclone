# ADR 0006: Phase 2 compiles eligible pure and read-only currents

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §4, §12 Phase 2
- Narrows: the reservation in ADR 0005 for `cyclone-ir`, `cyclone-compile`, and `cyclone-verify`

## Context

Phase 1 reproduces candidates from versioned traces and writes an audit. Eligibility under the provisional gates is a classification result. CTD-001 Phase 2 exits when eligible pure and read-only workflows pass equivalence tests.

A known filesystem effect can pass those gates. `unknown_side_effects` is false when the effect class is declared. Compilation needs a tighter rule than the classifier.

## Decision

Phase 2 adds three crates, edition 2021:

| Crate | Responsibility |
| --- | --- |
| `cyclone-ir` | `cyclone.cir/v0` and `validate`. |
| `cyclone-compile` | Guarded compilation to that form. A refusal is a value. |
| `cyclone-verify` | One digest lookup, then a shadow comparison against the source segments. |

The current's schedule is `interpret`. Its fallback is `original_agent_workflow`. The retry field is `3` when every step is `pure` and `1` when any step is `read_only`. The interpreter records that policy and performs one lookup.

Compilation requires promotion `eligible`, pressure `high` or `low`, determinism held, one environment digest, and a passed replay gate with positive economics. High pressure also requires a fixed parameter space and one table row. Low pressure accepts a fixed or bounded table. Every step effect is `pure` or `read_only`.

`cyclone compile` writes `cyclone.compile/v0`. `cyclone verify` writes `cyclone.verify/v0`. A verify report passes when every compiled current matches. A trace with nothing compiled passes with `checked` 0, and the process exits 0. A failed shadow exits 1.

`cyclone-runtime`, `cyclone-registry`, `cyclone-telemetry`, and `cyclone-tov` stay reserved. Live tool calls, filesystem and external mutation, and native codegen stay outside this phase. Repository Preflight stays a mechanical baseline.

## Consequences

`cyclone verify --evidence traces/reads-30.evidence.json traces/reads-30.jsonl` checks 30 read-only rows. `traces/pure-30.jsonl` compiles with idempotency `pure`. The Lucid fixture verifies with `checked` 0. An eligible `write_file` candidate is refused at the effect guard. Passing shadow equivalence leaves the current undeployed.
