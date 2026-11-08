# Cyclone

Cyclone is the loop-current detector, compiler, and adaptive execution runtime specified by [CTD-001](docs/ctd/CTD-001.md).

Phase 0 recorded the neighbouring contracts and the Repository Preflight baseline. Phase 1 ingests those journals, reconstructs causal order, and classifies candidate currents under the directive's provisional gates. Phase 2 compiles eligible pure and read-only currents into `cyclone.cir/v0` and checks shadow equivalence. Phase 3 schedules those currents as a typed graph and checks causal order and side effects. Phase 4 admits the journal under an independent lifecycle with canary routing, drift detection, rollback, and telemetry. Phase 5 registers those currents, discovers patterns shared by more than one agent, and reuses a tenant-neutral artifact under a fresh grant. The adversarial programme runs the twelve hostile scenarios from the directive through that journal and records containment. The operator dashboard projects that journal into the seven views of CTD-001 §11. Every compiled current declares an equivalence contract: exact output for a pure current, and an environment snapshot for a read-only current. Every compiled current records an execution guarantee: a pure current names a retry limit of three attempts, and a read-only current names a policy of one attempt. The journal performed one lookup inside that limit. Every compiled current records an authority context: the observed principals, the environment digest, and the signature operations. A consumer context names the grant principal. The Tov transport adapter remains unbuilt because Tov currently exposes a compiler CLI without a transport protocol; see [ADR 0002](docs/adr/0002-tov-toolchain-interface.md) for the adapter entry criteria. The reference workload stays unpromoted: its samples are mechanical, and inference cost is unmeasured.

| Record | Path |
| --- | --- |
| Directive | [docs/ctd/CTD-001.md](docs/ctd/CTD-001.md) |
| Phase 0 index | [docs/phase-0/README.md](docs/phase-0/README.md) |
| Decision records | [docs/adr/](docs/adr/0001-phase-0-holds-implementation.md) |
| Threat model | [docs/threat-model/phase-0.md](docs/threat-model/phase-0.md) |
| Reference workload | [docs/workload/repository-preflight.md](docs/workload/repository-preflight.md) |
| Baseline | [baseline/README.md](baseline/README.md) |
| Phase 1 | [docs/phase-1/README.md](docs/phase-1/README.md) |
| Phase 2 | [docs/phase-2/README.md](docs/phase-2/README.md) |
| Phase 3 | [docs/phase-3/README.md](docs/phase-3/README.md) |
| Phase 4 | [docs/phase-4/README.md](docs/phase-4/README.md) |
| Phase 5 | [docs/phase-5/README.md](docs/phase-5/README.md) |
| Adversarial | [docs/adversarial/README.md](docs/adversarial/README.md) |
| Dashboard | [docs/dashboard/README.md](docs/dashboard/README.md) |
| Equivalence | [docs/equivalence/README.md](docs/equivalence/README.md) |
| Guarantees | [docs/guarantees/README.md](docs/guarantees/README.md) |
| Authority | [docs/authority/README.md](docs/authority/README.md) |

The corpus lock binds six repository HEADs and their manifest digests:

```sh
deliver --spec deliver.toml --strict --allow-commands
```

Captured 2026-11-08. Twelve mechanical samples, median wall time 14.921 ms. Agent observations: 0. Promoted currents: 0.
