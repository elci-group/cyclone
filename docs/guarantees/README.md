# Execution guarantee

Declaration for CTD-001 §8.1. `cyclone guarantees` writes `cyclone.guarantees/v0` for currents the compiler emitted.

## Command

```sh
cyclone guarantees --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
```

The process exits 0 when `pass` is true. It exits 1 when a declared execution guarantee fails.

## Guarantees

A pure current records category `pure`, kind `retry_within_limit`, and an attempt limit of 3. A read-only current records category `read_only`, kind `policy_retry`, and an attempt limit of 1. A current that mixes a pure step with a read-only step uses the read-only guarantee. The journal performed one lookup, so `attempts_performed` is 1.

`exactly_once` stays false. `transactional` stays false. `reconciliation` stays false. The journal field is `memory`. The operation id is the current id.

Filesystem mutation, external mutation, and other non-idempotent effects stay unclaimed. The reason is `exactly-once unclaimed`. A refused candidate stays unclaimed. The execution guarantee records the attempt limit the compiler declared.

An empty journal passes with an empty guarantee list. Repository Preflight stays the mechanical Phase 0 baseline. Promoted currents stay 0. The authority context for a compiled current stays on `cyclone authority`.
