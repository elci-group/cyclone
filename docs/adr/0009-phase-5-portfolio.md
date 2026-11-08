# ADR 0009: Phase 5 reuses a current only under the consumer's authority

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §8, §9, §10, §12 Phase 5
- Narrows: the registry sentence reserved in ADR 0008

## Context

Phase 4 admits one tenant's journal. CTD-001 Phase 5 exits when reuse benefits are demonstrated without tenant leakage or authority escalation. Cross-tenant reuse requires a separately validated tenant-neutral implementation and fresh authorisation under the consuming tenant.

## Decision

`cyclone-registry` keeps compiled currents in a tenant-isolated registry and writes `cyclone.portfolio/v0`.

| Piece | Rule |
| --- | --- |
| Artifact | `cyclone.registry/v0`. The owning record keeps that tenant's agent ids and observation ids. |
| Pattern | Two or more agent ids inside one tenant, one signature, one environment. Mechanical sources stay out of the agent set. |
| Same-tenant reuse | The shared artifact is compiled once. The reuse benefit is the detection cost multiplied by the extra agents. |
| Economics | `V = N(C_o-C_c)-(C_d+C_v+C_m+C_r)` from `cyclone.economics/v0`. A non-positive value is refused. An absent entry stays unmeasured. |
| Federation | A neutral artifact id is `neutral:{signature}:{environment}`. Agent ids and observation ids stay empty. The source tenant is not part of the id. |
| Grant | `cyclone.grant/v0` names the consumer, the signature, and the operations. The consumer record is a new artifact under the consumer tenant. |
| Authority | Operations outside the artifact are an escalation and create no consumer record. |
| Isolation | A neutral or consumer record that carries another tenant's observation id, agent id, or tenant token fails the report. |

`cyclone portfolio` exits 1 when that isolation check fails. A refusal to reuse exits 0 and adds no benefit. `cyclone-tov` stays reserved. Live tool calls, mutation, native codegen, and a network registry stay outside this phase. Repository Preflight stays a mechanical baseline. A passing portfolio is journal evidence for reuse.

## Consequences

`traces/portfolio-agents.jsonl` discovers `alpha` and `beta` on `read`. With `traces/portfolio-read.economics.json` the reuse benefit is 20. Adding `traces/portfolio-consumer.grant.json` adopts the neutral artifact for `tenant-b` and raises the benefit to 40. The consumer record carries an empty observation list.
