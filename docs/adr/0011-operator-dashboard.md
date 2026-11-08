# ADR 0011: The operator dashboard records the journal

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §11
- Narrows: nothing in ADR 0001 through ADR 0010

## Context

The journal can classify, compile, shadow, schedule, admit, reuse, and contain a current. CTD-001 §11 requires an operator-facing execution and optimisation dashboard with seven views: current registry, cyclone graph, pressure history, compilation report, economic report, execution trace, and safety report.

## Decision

`cyclone dashboard` projects those existing records into `cyclone.dashboard/v0`. The projection reads the audit, compile, verify, schedule, admit, and portfolio results and adds the causal action list. Admission of a current stays with the admit command. The dashboard reads that result.

Deployment scope on a current is `journal` while the current is in the admission lifecycle, including a suspended current. A refused candidate and a compiled current absent from admission use scope `none`. The dashboard record itself is a journal record.

Pressure history states agreement as matching observations over considered observations. Compilation records the `interpret` schedule, the validation outcome, and the `sha256` of the canonical CIR JSON. Avoided inference stays unmeasured. Net value uses the portfolio formula `V_c = N_c(C_o-C_c)-(C_d+C_v+C_m+C_r)`. Break-even is the smallest execution count with a positive value. A non-positive per-execution saving has no break-even.

The page is one self-contained HTML document. The dashboard JSON is the record an exporter would read. OpenTelemetry export stays outside this gate.

A passing report means the projection finished, every current scope is journal or none, and the portfolio record did not leak. It is an operator record of the journal. Repository Preflight stays a mechanical baseline. Live tool calls, mutation, native codegen, a network registry, and `cyclone-tov` stay outside this gate.

## Consequences

`cyclone dashboard` reads a trace and exits 1 when the projection fails or the portfolio record leaked. Refused promotions stay visible on the safety view, and that report still passes. Promoted currents stay 0.
