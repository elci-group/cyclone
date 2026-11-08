# ADR 0013: Compiled currents record an execution guarantee

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §8.1
- Narrows: nothing in ADR 0001 through ADR 0012

## Context

CTD-001 §8.1 requires an explicit execution guarantee for each action type. Pure computations may be retried within a resource limit. Idempotent operations may be retried under a declared policy. Non-idempotent operations require an operation identifier, a durable execution journal, and reconciliation where those mechanisms exist. Exactly-once external effects stay unclaimed without a transactional mechanism.

The compiler emits pure and read-only currents. A pure current records idempotency `pure` and a limit of 3 attempts. A read-only current records idempotency `read_only` and a policy of 1 attempt. The interpreter performs one lookup and leaves that limit unread.

## Decision

`cyclone guarantees` writes `cyclone.guarantees/v0`.

| Effects | Category | Kind | Attempt limit | Attempts performed |
| --- | --- | --- | --- | --- |
| Every step pure | `pure` | `retry_within_limit` | 3 | 1 |
| Any read-only step, with the rest pure | `read_only` | `policy_retry` | 1 | 1 |

Each guarantee names the current id as its operation id. The journal field is `memory`. `reconciliation` stays false. `transactional` stays false. `exactly_once` stays false on the report, on each guarantee, and on each unclaimed row.

A current whose effects fall outside pure and read-only stays unclaimed with reason `exactly-once unclaimed`. A refused candidate stays unclaimed. The report passes when every declared guarantee matches its effects, the performed attempt stays inside the limit, the shadow comparison held, and exactly-once stayed false. An empty journal passes.

The Tov adapter boundary stays the Lucid path in ADR 0003. `cyclone-tov` stays reserved. OpenTelemetry, native codegen, and live tool calls stay outside this gate.

## Consequences

`cyclone guarantees` reads a trace and exits 1 when a declared guarantee fails. Promoted currents stay 0. Repository Preflight stays a mechanical baseline.
