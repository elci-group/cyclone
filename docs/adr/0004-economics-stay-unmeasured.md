# ADR 0004: Economics stay unmeasured for inference

- Status: accepted
- Date: 2026-11-08
- Directive: CTD-001 §3.3, §10, §13

## Context

Promotion requires a positive net value

`V_c = N_c (C_o - C_c) - (C_d + C_v + C_m + C_r)`

and the provisional observation gates (at least 30 independent successes for high pressure, at least 15 for low pressure). Those gates are engineering targets. They are not a proof of correctness.

## Decision

Phase 0 records the mechanical cost of Repository Preflight and leaves every inference term unmeasured. `V_c` is not computed. No current is promoted for speed or for savings.

| Term | Phase 0 value |
| --- | --- |
| `N_c` | Unknown. No eligible production volume is defined |
| `C_o` wall time | Measured. Median 14.921 ms across 12 samples. Min 11.091 ms. Max 24.31 ms |
| `C_o` inference | Unmeasured. The preflight called no model |
| `C_c`, `C_d`, `C_v`, `C_m`, `C_r` | Unmeasured. There is no compiled current |
| Agent observations | 0 |
| Promotion | Refused |

The 12 samples are repeated runs by one operator over six checkouts. They are benchmark replicates. They are not independent agent observations, so they do not count toward the gates of 15 and 30.

A later savings claim needs paired workloads with matched inputs, a stated horizon, monetary units, and the compilation and verification cost included. The synthetic Lucid fixture usage (`input_tokens` 1200, `output_tokens` 80) is not a price baseline. Its payload hashes are fixtures.

## Evidence

- `baseline/measurements/repository-preflight.json`, captured `2026-11-08T12:39:18Z`.
- Host: Linux 6.18.7-76061807-generic, x86_64, Python 3.12.3, rustc 1.99.0, git 2.43.0, tov 0.0.1, deliver 1.0.1.
- The procedure sets `network` false and `cargo_metadata` false. `cargo metadata` can reach the network and is outside this baseline.
- Process high-water mark `process_max_rss_kb` is 19400 for the interpreter. It is not a per-repository allocation.

## Consequences

Repository Preflight is too cheap, mechanically, for a compiler to pay for itself on wall time. The only plausible saving is avoided inference, and that quantity is still unmeasured. Economic promotion waits on paired agent traces. A negative or unknown `V_c` blocks promotion that is justified only by cost. Another documented operational objective can still justify compilation later, and that objective has to be written down before promotion.
