# ADR 0002: The Tov interface is the compiler CLI

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §8, `cyclone-tov`

## Context

CTD-001 asks for a Tov-native runtime and names Tov as the preferred ecosystem transport, subject to a defined wire protocol and interoperability tests. The local Tov checkout is a programming-language toolchain.

## Decision

Cyclone treats `/home/sal/Tov` as an external compiler. The supported interface is the `tov` CLI observed in `crates/tov/src/main.rs` and confirmed by `tov version`:

| Command | Role in a future current |
| --- | --- |
| `tov check [paths...] [--json]` | Read-only candidate. JSON diagnostics carry `code`, `file`, `start`, `message`, and `fixes` with an applicability of `safe` or otherwise. |
| `tov explain <CODE>` | Read-only lookup of a diagnostic code. |
| `tov test [path]` | Builds and runs `test(...)`. Effect class stays unknown until the test body is classified. |
| `tov build`, `tov run`, `tov clean` | Mutating or process-spawning. Out of the reference workload. |

`cyclone-tov` remains an adapter name. Phase 0 does not invent a wire schema for it. Agents and tools that do not call `tov` use the Lucid adapter boundary in ADR 0003.

A repeated `tov check` may later become a read-only current. Applying a compiler fix is a filesystem mutation whose choice of edit is generative. That step stays a gust until an equivalence contract exists.

## Evidence

- Package `tov` 0.0.1, edition 2024, workspace member `crates/tov`, HEAD `ba81eb0d9cffe745411cafdd112c59d86cd7deb8` on `main`, origin `https://github.com/richiemcilroy/Tov`.
- The crate declares zero Cargo dependencies. Eight integration tests are explicit `[[test]]` paths under `/home/sal/Tov/tests`, with `harness = false` on the member manifest.
- `tov version` printed `tov 0.0.1`.
- README and `AGENTS.md` describe a language, a native backend, and `tov check --json`. They do not describe a transport envelope, tenant, or capability field.
- Workspace `traci.toml` is a separate analysis config. Its `trace_id` field is a correlation key for that tool. It is not a Cyclone observation schema.

## Consequences

Interoperability tests for a Tov transport wait on a protocol. The first Tov-shaped workload Cyclone can shadow is `tov check --json` against a fixed tree, compared by diagnostic code and span, with fix application left on the agent path.

## Transport entry criteria

`cyclone-tov` remains unimplemented as a transport adapter. The observed CLI is not a transport contract, and Cyclone must not infer tenant identity, authority, or event ordering from compiler diagnostics. Work on that adapter can start once Tov publishes a versioned wire protocol and a reference implementation that Cyclone can test against. At minimum, the contract must define message framing and version negotiation, authenticated tenant and agent identity, operation and capability fields, causal identifiers and ordering, error and retry semantics, and replay/idempotency behavior. Interoperability tests must cover valid exchanges, malformed or unsupported messages, tenant isolation, and replay handling. Until those prerequisites exist, the CLI remains a separate read-only compiler interface and no transport behavior is claimed.
