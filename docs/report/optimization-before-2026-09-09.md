# Engineering optimization baseline — 2026-09-09

Evidence snapshot at `9df09f8`, before production changes. Non-authoritative.
Toolchain: rustc 1.97.1, x86_64-unknown-linux-gnu, WSL/Linux. Release profile:
opt-level 3, thin LTO, one codegen unit, symbols stripped, panic abort.
Pre-existing untracked package.json/package-lock.json were preserved.

## Structure

Workspace: four library crates, CLI composition root, xtask. Verified internal
DAG: core has no internal dependencies; fetch/adapters/state each depend only on
core; CLI depends on all four. Approximate production lines (excluding trailing
unit-test modules): core 2590, fetch 1324, adapters 2330, state 716, CLI 2136,
xtask 694. Largest mixed modules: fetch engine 636, adapter helpers 693,
scan engine 387. Date parser (670) and lifecycle updater (650) are cohesive
state machines and were inspected without planning a mechanical split.

## Checks

- cargo fmt --check: pass.
- cargo check --workspace --all-targets: pass.
- cargo xtask check: pass (registry, matrix, schema drift).
- cargo xtask baseline: pass, 401 tests, strict workspace/all-target/all-feature
  Clippy, fmt, existing RSS fixture probe.
- cargo build --release: pass.
- Initial sandbox attempt could not reach the configured crates.io proxy;
  authorized Cargo execution succeeded. No live sites are used by tests.

CI currently checks quality, acceptance, unsafe and supply chain. Release CI
checks static musl linkage, 30 MiB size, checksum and attestation; it does not
actually invoke the documented baseline. No existing dedup/full-pipeline timing
probe was found; the existing perf_rss example repeatedly processes 4000 events
and measured 6880 KiB peak RSS.

## New offline scaling probe, run against the unchanged implementation

`cargo run -p math_talk_radar --example perf_pipeline --release` uses a Clay
fixture-derived Event with deterministic synthetic identities. Dedup input
creation is excluded; timings are medians of three runs. Collision case: 10000
inputs into 10 clusters with fixed-size provenance. Pipeline: dedup, score,
sort, two redb scans, JSON and JSONL; excludes HTTP and registry enrichment.
It is a one-run pipeline diagnostic, not a claim about full network scan time.

```json
{
  "dedup_ms": {
    "1000": 13.245603,
    "10000": 1190.298003,
    "5000": 329.641557,
    "collision_10000": 27.385206
  },
  "json_bytes": 7895561,
  "json_ms": 19.842083,
  "jsonl_ms": 18.842662999999998,
  "method": "offline-v1",
  "peak_rss_kb": 91176,
  "pipeline_10000_ms": 1291.791972,
  "state_two_scans_ms": 178.248357
}
```

Startup: 30 fresh process invocations per flag, median wall time including
process creation; warm OS cache. Native GNU binary, not musl.

```json
{
  "--version_median_ms": 1.4790704990446102,
  "--help_median_ms": 1.4752849992873962,
  "binary_bytes": 11234328
}
```

## Confirmed defects

Interest-flip regression changes ID/title/URL/provenance on the old code.
Robots budget=1 regression fails the cache initializer with BudgetExhausted.
State scan deletes all absent events unconditionally; partial-source health is
not passed to state. Release abort invalidates task-panic isolation. Dedup has
quadratic cluster scans; fetch_all creates all source tasks and performs an
O(S²) missing-result scan. State clones the current corpus in addition to
materializing previous events, while CLI also retains cloned fetch candidates.
