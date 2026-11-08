# Operator dashboard

Operator record for CTD-001 §11. `cyclone dashboard` projects the journal into seven views and writes `cyclone.dashboard/v0`.

## Command

```sh
cyclone dashboard --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
cyclone dashboard --html dashboard.html --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
```

`--evidence`, `--policy`, `--economics`, and `--grant` apply. The process exits 0 when `pass` is true. It exits 1 when the projection fails or the portfolio record leaked.

## Views

The page carries these sections:

- Current registry: pressure, version, owner, evidence, status, and deployment scope.
- Cyclone graph: parent-child edges, activation triggers, and the critical path.
- Pressure history: transitions, agreement, and drift causes.
- Compilation report: the `interpret` schedule, validation, and the artifact hash.
- Economic report: avoided inference, net value, and break-even.
- Execution trace: causal action history and fallback events.
- Safety report: rejected promotions, policy violations, and suspended currents.

The pressure history view records agreement as matching observations over considered observations. Avoided inference stays unmeasured. Deployment scope is the journal for an admitted current and none for a refused or withheld current.

A passing report is an operator record of the journal. The JSON is the record an exporter would read. OpenTelemetry export stays outside this gate. The crate serves the page once on a loopback socket and reads it back.

Repository Preflight stays the mechanical Phase 0 baseline. Promoted currents stay 0. The equivalence contract for a compiled current stays on `cyclone equivalence`.
