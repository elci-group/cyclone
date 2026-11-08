# Repository Preflight baseline

- Document: `baseline/measurements/repository-preflight.json`
- Schema: `cyclone.baseline.repository-preflight/v0`
- Captured: `2026-11-08T12:39:18Z`
- Lock: `baseline/corpus.lock.json`

One operator ran the procedure twice on six local repositories. The runs are mechanical. They called no model.

| Quantity | Value |
| --- | --- |
| Samples | 12 |
| Wall time min | 11.091 ms |
| Wall time median | 14.921 ms |
| Wall time max | 24.31 ms |
| Interpreter high-water RSS | 19400 KB |
| Agent observations | 0 |
| Inference cost | unmeasured |
| `V_c` | unmeasured |
| Promotion | refused |

Per-repository wall times, run 1 then run 2, in milliseconds:

| Repository | Run 1 | Run 2 |
| --- | ---: | ---: |
| Tov | 15.118 | 15.720 |
| lastdown | 20.060 | 14.725 |
| deliver | 24.31 | 14.435 |
| lucid-cli | 23.223 | 11.091 |
| bank | 18.099 | 11.226 |
| padagonia | 14.362 | 13.737 |

Later runs are often faster. That difference is cache behaviour on this host. It is not a compiled current.

CPU time from `getrusage` sits within a few milliseconds of wall time for these short `git` calls. `process_max_rss_kb` is the Python process high-water mark and is the same figure on every sample.

Host: Linux 6.18.7-76061807-generic, x86_64, Python 3.12.3, rustc 1.99.0 (b940084d7 2026-09-28), git 2.43.0, tov 0.0.1, deliver 1.0.1.

Reproduce the lock check:

```sh
python3 baseline/preflight.py --check baseline/corpus.lock.json
```

A fresh measurement overwrites the JSON and the lock. Review the diff before replacing the captured baseline. The numbers above describe the 2026-11-08 capture only.
