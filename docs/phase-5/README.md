# Phase 5

Portfolio reuse. Exit condition from CTD-001: reuse benefits are demonstrated without tenant leakage or authority escalation.

## Commands

`cyclone portfolio` compiles the trace, registers each current under its tenant, and writes `cyclone.portfolio/v0`. A second agent in that tenant reuses the compiled artifact. A consumer in another tenant receives a neutral artifact only when a `cyclone.grant/v0` file names that consumer and limits operations to the artifact.

```sh
cyclone portfolio --json --evidence traces/reads-30.evidence.json --economics traces/portfolio-read.economics.json traces/portfolio-agents.jsonl
cyclone portfolio --json --evidence traces/reads-30.evidence.json --economics traces/portfolio-read.economics.json --grant traces/portfolio-consumer.grant.json traces/portfolio-agents.jsonl
cyclone portfolio --json traces/lucid-session-v1.jsonl
```

The process exits 0 when `pass` is true. It exits 1 when a neutral or consumer record carries another tenant's identity. A missing grant, a non-positive value, or an operation outside the artifact is a refusal with benefit 0.

## Shared economics

The paired workload uses `V = N(C_o-C_c)-(C_d+C_v+C_m+C_r)`. On the read entry, `N` is 30, the per-execution saving is 9, and the overhead is 30, so `V` is 240. Two agents share one detection cost of 20, so the reuse benefit is 20. The grant adds a second avoided detection cost and the reuse benefit is 40.

`traces/portfolio-agents.jsonl` holds 15 executions from `alpha` and 15 from `beta`, the same read digest, tenant `tenant-a`. The pattern lists both agents. The consumer artifact for `tenant-b` keeps empty agent and observation lists. Its id does not name `tenant-a`.

Repository Preflight stays the mechanical Phase 0 baseline. A passing portfolio is journal evidence for reuse. The registry here is that journal's artifact list. External effects stay outside this phase. The adversarial programme, recorded in ADR 0010, contains the twelve hostile scenarios.
