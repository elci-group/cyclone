# ADR 0012: Compiled currents declare an equivalence contract

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §7, §7.1
- Narrows: nothing in ADR 0001 through ADR 0011

## Context

CTD-001 §7 requires every compiled current to declare an equivalence contract. §7.1 names pure computation, read-only workflows, filesystem mutation, external mutation, time-dependent work, and non-deterministic work. The compiler emits pure and read-only currents. Shadow comparison checks exact outputs under one environment digest.

## Decision

`cyclone equivalence` writes `cyclone.equivalence/v0`.

| Effects | Category | Check | Tolerance |
| --- | --- | --- | --- |
| Every step pure | `pure` | `exact_output` | `none` |
| Any read-only step, with the rest pure | `read_only` | `environment_snapshot` | `none` |

The shadow counts are copied onto that contract. `irreversible_replay` stays false. Equivalence performs one lookup. The interpreter leaves external actions on the original workflow.

A current whose effects fall outside pure and read-only stays undeclared. A refused candidate stays undeclared. The report passes when every declared contract matches its effects, the shadow comparison held, tolerance is `none`, and irreversible replay stayed false. An empty journal passes with an empty contract list.

Exactly-once external effects stay unclaimed. Repository Preflight stays a mechanical baseline. Execution guarantees, OpenTelemetry, `cyclone-tov`, native codegen, and live tool calls stay outside this gate.

## Consequences

`cyclone equivalence` reads a trace and exits 1 when a declared contract fails its shadow comparison. Promoted currents stay 0.
