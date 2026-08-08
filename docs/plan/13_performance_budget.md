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
