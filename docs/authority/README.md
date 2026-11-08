# Authority context

Declaration for CTD-001 §9. `cyclone authority` writes `cyclone.authority/v0` for currents the compiler emitted.

## Command

```sh
cyclone authority --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
```

The process exits 0 when `pass` is true. It exits 1 when a compiled current has no principal or a context elevates the workflow.

## Contexts

An owner context names the current's tenant, the observed agent principals, the environment digest as the permitted resource, and the signature as the allowed operations. A consumer context requires `cyclone.grant/v0`. Its principal is the grant principal. Its operations stay inside the signature.

`elevated` stays false. `historical_permission` stays false. The authority context records the tenant, the principals, the permitted resource, and the allowed operations.

A privileged effect stays unauthorised. A refused candidate stays unauthorised. An empty journal passes with an empty context list. Repository Preflight stays the mechanical Phase 0 baseline. Promoted currents stay 0.
