# Phase 2

Current compiler. Exit condition from CTD-001: eligible pure and read-only workflows pass equivalence tests.

## Commands

`cyclone audit` classifies a trace under the provisional gates.

`cyclone compile` writes `cyclone.compile/v0`. Each eligible pure or read-only candidate becomes one `cyclone.cir/v0` value. Anything else is a refusal with a reason.

`cyclone verify` runs that current as a digest lookup and writes `cyclone.verify/v0`. The process exits 0 when `pass` is true. It exits 1 when a compiled current fails the shadow comparison.

```sh
cyclone compile --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
cyclone verify --json --evidence traces/reads-30.evidence.json traces/reads-30.jsonl
cyclone verify --json --evidence traces/pure-30.evidence.json traces/pure-30.jsonl
cyclone verify --json traces/lucid-session-v1.jsonl
```

## Intermediate form

Schema `cyclone.cir/v0`. The schedule is `interpret`. The fallback is `original_agent_workflow`. The table maps input digests to output digests under one environment digest.

Retry is recorded on the current: 3 when every step is `pure`, 1 when a step is `read_only`. The interpreter performs one lookup. A missing input, or an environment other than the precondition, resolves to the original agent workflow and yields no output.

## What compiles

| Guard | Required value |
| --- | --- |
| Promotion | eligible |
| Pressure | high, or low |
| Effects | pure, or read_only, on every step |
| Determinism | one output tuple per input tuple |
| Environment | one digest |
| Parameters | fixed for high pressure; fixed or bounded for low |
| Replay | passed |
| Economics | attached value > 0 |
| Table | one row when pressure is high |

A known filesystem effect can be eligible in Phase 1. The compiler refuses it. Validation failure is returned as `validation:` plus the type error.

## Shadow

Each source segment is compared with the lookup. A fallback counts as a mismatch. `pass` is true when every compiled current matches, including the case where the compiled list is empty and `checked` is 0. The empty case means the refusal is the recorded result.

## Versioned traces

| File | What it shows |
| --- | --- |
| `traces/reads-30.jsonl` | Thirty identical agent reads. Compiles when the evidence file is attached. Shadow matches 30. |
| `traces/pure-30.jsonl` | Thirty identical pure steps. Idempotency `pure`, retry 3. |
| `traces/pure-30.evidence.json` | Passed replay and economic term `1` for signature `pure`. |
| `traces/lucid-session-v1.jsonl` | No compilable segment. Verify passes with `checked` 0. |
| `traces/environment-split.jsonl` | Two environment digests. Compile refuses the candidate. |

Repository Preflight's Phase 0 baseline is unchanged. Those samples are mechanical, and this phase leaves them unpromoted. A passing shadow check is equivalence evidence. Deployment stays in a later phase. Phase 3, recorded in ADR 0007, schedules the compiled currents.
