# ADR 0010: Adversarial traces stay contained

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §14, §3.3, §5.2
- Narrows: nothing in ADR 0001 through ADR 0009

## Context

The journal can classify, compile, shadow, schedule, admit, and reuse a current. CTD-001 §14 requires a verification suite of deliberately hostile workflow evidence, with property-based checks, fuzzing, differential execution, fault injection, and controlled traces.

## Decision

`cyclone-adversarial` runs that programme in-process and writes `cyclone.adversarial/v0`. The corpus file names the twelve scenarios. Expected containment lives in the programme. A corpus that omits a scenario is rejected.

| Scenario | Containment |
| --- | --- |
| environment | Split environments refuse promotion. A later environment mismatch falls back. |
| idempotency | A repeated filesystem mutation stays uncompiled. |
| injection | `model.response` stays outside every signature. The repeated mutation stays uncompiled. |
| fan_in | The child waits until both parents have completed. |
| dependency | An environment that changed after observation rolls back onto the original workflow. |
| authority | A cross-tenant candidate is refused. A consumer without a grant is refused. An extra operation is an authority escalation. |
| partial | A failed step stays outside the compiled chain. |
| cache | Equal inputs with different outputs stay unshared. |
| cancel | A zero deadline cancels the child before interpretation. |
| rare | A second observed input stays at low pressure. An input absent from the table falls back. The observed rare row stays in the shadow comparison. |
| economics | A non-positive value stays unadopted. |
| objectives | Matching `pay` sequences with different outputs, and `pay` steps with an external effect, stay unconsolidated. |

Property, fuzz, differential, and fault probes use the same guards and add no crate. A passing report is journal evidence that these traces were contained. It is a containment record. Repository Preflight stays a mechanical baseline. `cyclone-tov`, the dashboard, and OpenTelemetry stay outside this gate. Live tool calls, mutation, native codegen, and a network registry stay outside this gate.

## Consequences

`cyclone adversarial` reads `traces/adversarial.json` and exits 1 when containment fails. The twelve scenarios report contained. `leaked` stays false. `executable_effects` stays 0.
