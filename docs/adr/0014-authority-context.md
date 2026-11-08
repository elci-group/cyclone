# ADR 0014: Compiled currents record an authority context

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §9
- Narrows: nothing in ADR 0001 through ADR 0013

## Context

CTD-001 §9 requires every current to execute under an explicit authority context. The context defines the tenant, the principal, the permitted resources, and the allowed operations. The compiler keeps the permissions of the observed workflow. Cross-tenant reuse already requires a fresh grant under ADR 0009.

## Decision

`cyclone authority` writes `cyclone.authority/v0`.

| Binding | Tenant | Principals | Resource | Operations |
| --- | --- | --- | --- | --- |
| `owner` | The current's tenant | Observed agent ids for that signature | The environment digest | The signature |
| `consumer` | The grant's consumer tenant | The grant principal | The same environment digest | The granted operations that already belong to the signature |

`elevated` stays false. `historical_permission` stays false. A privileged effect stays unauthorised. A compiled current with no observed principal stays unauthorised and the report fails. An extra granted operation stays unauthorised and creates no consumer context. A consumer principal that repeats a source agent stays unauthorised. The consumer context leaves the source tenant and the source agent ids off the record.

An empty journal passes with an empty context list. A refused candidate stays unauthorised.

Signed artifacts, execution quotas, OpenTelemetry, `cyclone-tov`, native codegen, and live tool calls stay outside this gate.

## Consequences

`cyclone authority` reads a trace and exits 1 when a compiled current has no principal or a context elevates the workflow. Promoted currents stay 0. Repository Preflight stays a mechanical baseline.
