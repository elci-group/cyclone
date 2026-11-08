# Phase 0

Recorded 2026-11-08. CTD-001 is the proposed architecture baseline in [docs/ctd/CTD-001.md](../ctd/CTD-001.md). These notes are the inspection that the directive requires before implementation.

## What was inspected

| Path | Finding |
| --- | --- |
| `/home/sal/cyclone` | Empty at the start of the phase |
| `/home/sal/Tov` | Compiler toolchain, `tov` 0.0.1. CLI interface. No transport envelope |
| `/home/sal/lucid-cli/contracts/v1` | Event, command, receipt, evidence, snapshot, and `lucid.session/v1` |
| `/home/sal/lucid-cli/crates/lucid-contract` | Action vocabulary, redaction, sequence numbering |
| `/home/sal/lastdown` | Rust 2021 continuation crate 0.1.6. Absolute path dependency on `/home/sal/3form` |
| `/home/sal/deliver` | Deliverable gate 1.0.1, used as this phase's check |
| `/home/sal/bank` | Rust 2021, sibling path dependency `../3form` |
| `/home/sal/padagonia` | Claim kinds `Assertion` and `Observation`. Version 0.2.2. Not linked |
| `/home/sal/toungsten` | Empty directory. `git` walks up to `/home/sal` |

## Deliverables

| CTD-001 Phase 0 item | Record |
| --- | --- |
| Architecture decision records | [0001](../adr/0001-phase-0-holds-implementation.md), [0002](../adr/0002-tov-toolchain-interface.md), [0003](../adr/0003-lucid-session-observations.md), [0004](../adr/0004-economics-stay-unmeasured.md) |
| Workload corpus | [Repository Preflight](../workload/repository-preflight.md), `baseline/corpus.lock.json` |
| Threat model | [phase-0.md](../threat-model/phase-0.md) |
| Baseline cost measurements | [baseline/README.md](../../baseline/README.md) |

Agent observations in the corpus: 0. The pressure gates are unmet on this corpus. Phase 1, recorded in ADR 0005, classifies later traces with these same provisional gates. Phase 2, recorded in ADR 0006, compiles eligible pure and read-only currents and checks them in shadow. Phase 3, recorded in ADR 0007, schedules those currents as a typed graph. Phase 4, recorded in ADR 0008, admits compiled currents under an independent lifecycle. Phase 5, recorded in ADR 0009, reuses a current under the consumer's own authority. The adversarial programme, recorded in ADR 0010, contains the twelve hostile scenarios. The operator dashboard, recorded in ADR 0011, projects the journal into the seven views. The equivalence contract, recorded in ADR 0012, names the shadow check for each compiled current. The execution guarantee, recorded in ADR 0013, names the attempt limit for each compiled current. The authority context, recorded in ADR 0014, names the tenant, principal, resource, and operations for each compiled current.

## Still required before Phase 1

Approval of the security and equivalence policies. A wire protocol, if `cyclone-tov` is going to be a transport rather than a compiler adapter. Paired agent traces before any savings claim. Held-out repositories before shadow mode.
