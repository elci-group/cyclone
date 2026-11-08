# Workload: Repository Preflight

- Schema: `repository-preflight/v0`
- Directive: CTD-001 §15
- Privilege: read-only. No production mutation
- Status: hypothesis. Promotion refused

## Procedure

The canonical segment, in order:

1. `git.toplevel`
2. `git.head`
3. `git.branch`
4. `git.origin`
5. `git.status_count` (tracked paths only; paths are discarded)
6. `manifest.discover`
7. `deps.extract` (names, path dependencies, edition, version)
8. `tests.discover` (explicit `[[test]]` targets and Cargo `tests/*.rs` or `tests/*/main.rs` when the manifest has a package)

Pruned from discovery: virtual manifests do not autodiscover tests. Workspace `edition` and `version` are inherited when a member sets `workspace = true`. `cargo metadata` is not run. The network is not used. File bodies other than manifests are not stored. Test files are stat'd, not read.

The implementation is `baseline/preflight.py`. The lock is `baseline/corpus.lock.json`.

## Corpus

Captured `2026-11-08T12:39:18Z`. Each toplevel matched the path that was asked for.

| Repository | HEAD | Branch | Edition | Leaving path dependencies | Tracked dirty | Tests |
| --- | --- | --- | --- | --- | --- | --- |
| `/home/sal/Tov` | `ba81eb0d9cffe745411cafdd112c59d86cd7deb8` | `main` | 2024 | none | 0 | 8 declared |
| `/home/sal/lastdown` | `e1c4a4e6ee7567133b3762a25dd9dd517660a4da` | `main` | 2021 | `form3` → `/home/sal/3form` | 0 | 8 |
| `/home/sal/deliver` | `1a1d473df4732f7e64edeee306b366d5bcd85c37` | `drowzy` | 2021 | none | 0 | 2 |
| `/home/sal/lucid-cli` | `e10024bfecf3d4ee6846aced5282a9ec55f148fc` | `main` | 2021 | `colony-core`, `colony-planner`, `mesut` (absolute); in-workspace paths for its own crates | 12 | 1 |
| `/home/sal/bank` | `9ba841a01d9c98294860f249a865b8eb664e48a1` | `main` | 2021 | `form3` → `../3form` | 3 | 1 |
| `/home/sal/padagonia` | `151df6ea63da43253ad02d6f48238cf587ed2a02` | `main` | 2021 | `bound-core` → `../bound/crates/bound-core` | 8 | 14 |

Lucid has no `origin` remote. Deliver's branch name is `drowzy`. Tov's member crate declares zero Cargo dependencies. Manifest sha256 values live in the lock and are rechecked by `deliver`.

## Provisional classification

This is a hypothesis for auditors. It is not a pressure label.

| Candidate | Class to test later | Why it is not promoted now |
| --- | --- | --- |
| The eight-step signature, parameterised by repository root | Low pressure | Argument digest, snapshot binding, and agent traces are missing. 0 agent observations. Gate is 15 |
| `git` identity given a fixed root and a clean index | High pressure | Environment digest omits path dependencies and dirty patches. Gate is 30. Dirty counts are already non-zero on three roots |
| Dependency and test discovery | Low pressure, typed by manifest shape | Tov's explicit tests and the other crates' autodiscovery are different shapes inside one signature |
| Interpreting what the repository is for | Gust | That step is generative |
| `tov check --json` | Separate read-only candidate | Not part of this procedure's samples |

Unknown side effects are prohibited for both pressure classes. `git status` as implemented here is read-only. A future step that runs tests is unknown, not read-only.

## Shadow-mode entry, when policy allows

Held-out repositories, disjoint from these six, run the original agent workflow and this procedure on the same snapshot. Compare HEAD, manifest digests, dependency names, and test paths. Latency and inference are reported as a pair. A wall-time win on the mechanical script does not count as the §13 latency target, because the original workflow's inference cost is still unmeasured.
