# 13 — Performance Budget

> Status: M0 skeleton. Authoritative for performance. §48.

## Binary size

Release target: ≤ 20 MiB preferred, ≤ 30 MiB hard. Exceeding 30 MiB requires a
written analysis.

## Memory

Offline benchmark: peak RSS ≤ 128 MiB. Live target: ≤ 200 MiB.

## Startup (§48)

`--version` / `--help` perform no network/state initialization. Target < 100ms.

## Scan

Real network: global default deadline = 30s. On timeout, return completed
results rather than waiting indefinitely.

## Determinism (§49)

Same fixture + config + `--today` must produce stable event ordering, IDs,
scores, and dedup. `generated_at` and runtime duration may vary; golden
comparisons ignore those.

## Acceptance cases

- PERF-001 — offline RSS ≤128 MiB (baseline).

## Regression evidence

`cargo xtask perf` generates `target/perf-latest.json`; CI archives it. Keep the
existing 4000-event RSS fixture memory probe. The offline-v1 workload measures
1k/5k/10k distinct and 10k fixed-provenance collision dedup (three-run medians),
10k dedup/rank/two-state-scan/JSON/JSONL processing, and peak RSS. The mock-scan-v1
probe drives the real run_scan pipeline twice over 20 local sources/400 events,
including registry enrichment, state and public rendering; it also verifies
source panic isolation in an actual release binary (tests alone force unwind).
The storage pipeline also retains the full 1k/5k/10k write/reopen/JSON/JSONL
workloads and the growing-provenance high-collision workload. Its final peak
RSS is checked against the same 128 MiB limit; all required metrics must exist.

Hard gates: each offline probe ≤128 MiB RSS, binary ≤30 MiB, startup median
≤1 s, 10k distinct/collision dedup ≤5 s. Startup <100 ms remains the target;
only catastrophic regressions gate shared runners. Other millisecond metrics
are advisory. Deterministic key-operation unit tests protect algorithmic growth.
Build/setup is excluded from timings, and no live-site performance is asserted.
