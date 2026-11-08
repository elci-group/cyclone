# Equivalence contract

Declaration for CTD-001 §7. `cyclone equivalence` writes `cyclone.equivalence/v0` for currents the compiler emitted.

## Command

```sh
cyclone equivalence --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
```

The process exits 0 when `pass` is true. It exits 1 when a declared contract fails its shadow comparison.

## Contracts

A pure current declares category `pure`, check `exact_output`, and tolerance `none`. A read-only current declares category `read_only`, check `environment_snapshot`, and tolerance `none`. A current that mixes a pure step with a read-only step uses the read-only contract. `irreversible_replay` stays false.

Filesystem mutation, external mutation, time-dependent work, and non-deterministic work stay undeclared. A refused candidate stays undeclared. The equivalence contract records the check the shadow interpreter performed.

An empty journal passes with an empty contract list. Repository Preflight stays the mechanical Phase 0 baseline. Promoted currents stay 0. The execution guarantee for a compiled current stays on `cyclone guarantees`.
