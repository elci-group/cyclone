# ADR 0001: Phase 0 holds the runtime

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §12 Phase 0, §17, §18

## Context

CTD-001 defines ten Rust crates, a typed current intermediate representation, and a production runtime. Its own delivery programme requires repository reconnaissance, interface validation, a workload corpus, a threat model, and baseline cost measurements before new abstractions.

The Cyclone directory was empty at the start of this phase. Neighbouring checkouts already carry execution contracts, a compiler toolchain, a deliverable gate, and an epistemic claim vocabulary.

## Decision

Phase 0 publishes the directive, four decision records, the threat model, the Repository Preflight corpus, and a measured baseline. It does not create a Cargo workspace.

The crate names in CTD-001 §8 stay reserved for Phase 1:

`cyclone-observe`, `cyclone-detect`, `cyclone-pressure`, `cyclone-ir`, `cyclone-compile`, `cyclone-runtime`, `cyclone-verify`, `cyclone-registry`, `cyclone-telemetry`, `cyclone-tov`.

When those crates are created, they use Rust edition 2021, matching Lastdown, Deliver, Lucid, Bank, and Padagonia on this machine (`rustc 1.99.0`). Tov's compiler crate is edition 2024 because it is the Tov compiler. Cyclone does not inherit that edition.

## Evidence

- `/home/sal/cyclone` had no files before this record.
- `/home/sal/Tov` is Tov 0.0.1, one workspace member, edition 2024, zero Cargo dependencies.
- `/home/sal/lucid-cli/contracts/v1` already defines event, command, receipt, evidence, and snapshot documents, plus a redacted `lucid.session/v1` journal.
- `deliver 1.0.1` is the local acceptance gate. This checkout's `deliver.toml` checks the Phase 0 files and the corpus lock.
- Padagonia 0.2.2 distinguishes `ClaimKind::Assertion` from `ClaimKind::Observation`. Cyclone does not take a path dependency on it in Phase 0.
- `/home/sal/toungsten` is an empty directory. `git -C` there walks up to `/home/sal`. An empty directory is not a workload.

## Consequences

Phase 1 starts only after the security and equivalence policies are approved. A Tov transport adapter stays reserved until a wire protocol exists to test against. The preflight script is a measurement tool. It is not `cyclone-observe`.
