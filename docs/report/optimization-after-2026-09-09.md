# Engineering optimization acceptance — 2026-09-09

Evidence only. Implementation commit: `9bf06db`; baseline revision: `9df09f8`.
Machine receipt: `optimization-after-2026-09-09.json`; methodology and previous
measurements: `optimization-before-2026-09-09.md`. Timings are local observations,
not statistical guarantees or live-site latency claims.

## Changes and architecture

- Ranking-dependent scalar selection replaced by stable ID/URL/provenance
  ordering. Enrich → dedup → state reconciliation → rank once → output.
- Greedy dedup uses exact-key indices and earliest cluster membership, preserving
  non-transitive semantics. Expected O(n log n + k log n) for n events and k
  identity-key operations, replacing the distinct-event O(n²) scan. Growing
  provenance uses incremental sets; caches are allocated only for merged clusters.
- Robots initialization no longer charges one source's logical content budget.
  Physical robots transactions remain origin/allowlist cached and independently
  bounded. Source task admission is O(jobs); result sorting is O(S log S), with
  O(1) expected task-ID failure lookup rather than O(S²) missing-source scans.
- Fixed same-host redirect permit deadlock, extended request timeout, empty
  native-ID false merges and inline enrichment continuing past the deadline.
- State owns the current vector, stamps it in place, and cancels only when all
  previous supporting sources are authoritative. Missing provenance/media/talks
  remain conservative on partial observations. Current candidate aliases transfer
  earliest first_seen and tombstones transactionally. Stored schema v2 and JSON
  1.0 remain compatible; historical aliases are never guessed.
- Source orchestration, HTTP transactions, identity extraction, merge logic,
  scan configuration/enrichment, media recognition and selector caches have clear
  owners. No generic pipeline, new third-party package, unsafe or reverse DAG edge.
- Candidate construction capped at 2001 (2000 retained + overflow sentinel),
  runtime selector cache at 256 per thread. Narrowed Tokio and removed unused
  dependency declarations; xtask uses existing serde_json for metadata/metrics.
- CI archives advisory metrics and enforces broad resource/catastrophic gates;
  release packaging invokes baseline and tests unwind isolation in a real
  release example. Historical baseline document is explicitly dated.

## Measured before → after

| Metric | Before | After |
|---|---:|---:|
| Dedup 1k | 13.25 ms | 2.39 ms |
| Dedup 5k | 329.64 ms | 15.05 ms |
| Dedup 10k | 1190.30 ms | 38.28 ms |
| Collision 10k / 10 clusters | 27.39 ms | 28.86 ms |
| Offline processing 10k | 1291.79 ms | 224.45 ms |
| Two state scans | 178.25 ms | 160.97 ms |
| Processing peak RSS | 91176 KiB | 70548 KiB |
| RSS adapter peak | 6880 KiB | 6860 KiB |
| GNU release binary | 11234328 B | 12190760 B |
| --help startup median | 1.48 ms | 1.41 ms |
| --version startup median | 1.48 ms | 1.45 ms |

Collision timing increased about 5% in this sample; three-run medians do not
establish statistical significance. The binary grows 8.5% to support production
unwinding, staying below 30 MiB. Startup is essentially unchanged. The offline
10k processing probe excludes HTTP and registry matching. Separately, actual
run_scan over 20 mock sources / 400 events, twice plus public rendering, takes
988.29 ms and 31296 KiB RSS. No pre-change measurement exists for that new probe.

## Validation and regression evidence

Passed: cargo fmt --check; cargo check --workspace --all-targets;
cargo test --workspace (425 tests, previously 401); strict Clippy with
--workspace --all-targets --all-features -- -D warnings; cargo xtask check;
cargo xtask check-matrix; cargo xtask baseline (including perf); cargo build
--release; git diff --check. Targeted subsystem tests preceded full validation.
Final unused-manifest cleanup passed fmt, workspace check and xtask check again.

New tests cover interest-only state invariance, equal-ID source ties, 1200 seeded
differential corpora (including media/talk collections), 1k/5k/10k key-operation
bounds, growing provenance, stale keys/bridges, empty native IDs, shared robots
budgets with both initializers and contention, bounded source admission, panic
and cancellation isolation, redirect permits, deadline waits/inline parsing,
per-event authority over multiple scans, owned allocation reuse, alias history,
selector retention limits and all implemented adapters' candidate bounds.

Unavailable locally: cargo-deny and cargo-llvm-cov (not installed; commands were
attempted). Musl static build/linkage not run: no musl target or compiler.
Remote CI and deployment were not executed. Existing xtask warning remains:
media/recording source coverage requires at least 3, currently 0.

## Remaining debt

- P0: no known unresolved defect in the targeted invariants after this audit;
  this is not a claim of exhaustive correctness.
- P1: source coverage and currently unimplemented adapter capabilities remain
  existing product limitations, outside this optimization's fixture-only scope.
- P2: previous state is still materialized; persisted ranking fields remain for
  compatibility; inline JSON-LD can repeatedly parse the same bounded document;
  parser DOM/feed allocation is bounded by input size, not a streaming design.
  Source-health history remains the separate draft ADR-0010. Static/supply-chain/
  coverage verification needs the missing tools. Existing CI action references
  remain version tags rather than full SHA pins.
- Intentionally deferred: destructive state compaction/schema projection,
  speculative caches, blanket interning, and replacement of the parser/storage
  stack. Old canonical candidates unavailable this scan receive conservative
  retention, not guessed identity migration.

Assessment (0–10): correctness 9; architecture 9; cohesion/coupling 9;
maintainability 8.5; determinism 9; performance 8.5; memory 8; complexity 9;
tests 8.5; deployability 7.5. Strong offline invariants and simpler ownership,
with remaining whole-corpus/parser costs and deployment verification gaps.

All task changes are committed locally on main. No push, installation or remote
publication. Pre-existing package.json/package-lock.json remain untracked.
