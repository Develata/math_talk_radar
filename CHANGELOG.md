# Changelog

All notable changes to `math_talk_radar` are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [0.1.0] — 2026-08-23

First public release, rewritten before the v0.1.0 tag/Release is finalized.
Pure Rust CLI for discovering public mathematics conferences, talks, lecture
series, recordings, slides, and related resources. No LLM, no browser
automation, no JS runtime.

### Security

- Update the locked `h2` dependency from 0.4.15 to 0.4.16 to address
  RUSTSEC-2026-0258 (unbounded empty HTTP/2 DATA frame queuing).
- Require a matching install manifest (or explicit `--force-unmanaged`) for
  update/uninstall in every directory. Custom Cargo target/profile paths are
  protected; lifecycle regression tests operate on temporary executable copies.

### Engineering optimization — 2026-09-09

- Ranking no longer selects canonical events or changes persistent identity;
  score once after dedup and state reconciliation.
- Shared robots requests have a separate bounded system transaction, independent
  of the initializing source's content budget. Fix redirect host-permit deadlock
  and request timeouts extended by a later global deadline.
- Hash-indexed first-cluster dedup replaces quadratic distinct-event scans;
  incremental provenance union avoids repeatedly rebuilding growing clusters.
  Empty native IDs no longer merge unrelated events. Inline enrichment checks
  the deadline between parser calls.
- Source task admission is bounded by jobs, with complete deterministic health
  reporting and release-profile panic isolation (unwind).
- Owned state scans preserve failed-source provenance, require all supplied source
  statuses to be Ok before any absence cancellation, and transfer first_seen through current dedup
  aliases. State schema v4 retains transactional migration; public JSON stays 1.0.
- Candidate materialization and selector caches have explicit bounds. Narrow
  Tokio features and remove unused dependency declarations.
- Add differential/invariant tests, Cargo-metadata architecture gates, offline
  performance JSON artifacts and real release-panic probes. Release packaging
  now runs the offline baseline; historical reports are labeled as snapshots.
- Accept valid musl static PIE in the linkage gate while rejecting dynamic ELF
  and ambiguous tool output. Release CI installs musl-tools and uses the same
  xtask linkage validator as local builds.


### Added

- Rust 2024 workspace: `radar-core` (pure domain), `radar-fetch` (HTTP),
  `radar-adapters` (pure document parsing), `radar-state` (persistence),
  `apps/cli` (composition root), `xtask` (dev tooling).
- Strict crate DAG enforced at compile time (§11); `#![forbid(unsafe_code)]`
  in every crate.
- Domain model: `Event`, `Talk`, `MediaResource`, `PersonHit`, `SourceSpec`,
  `FetchedDocument`, `FetchPlan`, `EventStub`, `EventCandidate`,
  `SourceHealth`, `ChangeRecord`.
- 6 source adapters: RSS, ICS, JSON-LD, Indico, HTML-config (CSS selectors),
  HTML-generic (fallback). All resolve relative links against the
  post-redirect `final_url` (H04).
- Fetch client with rustls TLS, HTTP policy, retry, robots (RFC 9309),
  per-source + global request budgets, global scan deadline.
- State schema v4 with change detection: event added/updated/cancelled,
  tombstone retention, source-health history, collision-safe change-log keys,
  and fixed-width ordered timestamps (ADR-0011, ADR-0013). Transactional
  v1→v2→v3→v4 migration fails closed on malformed rows or migration key
  collisions.
- CLI surface (§27): `scan`, `sources list`, `doctor`, `update`, `uninstall`,
  `schema`. Interactive TTY uninstall prompt (§35.1) with non-TTY refusal
  (§35.2). `--dry-run` is zero-mutation (R3-P0-04).
- Self-update: SHA-256 verification, rollback copy, download timeout + size
  caps, symlink-rejection at rollback path, update lock shared with uninstall.
- Output schema `1.0` (§64): `schemars`-derived JSON Schema with a golden-file
  drift test and an immutable v1.0 backward-compatibility gate (R3-P1-03).
- xtask validators: `check` (source-registry + acceptance-matrix + doc
  coverage), `check-matrix` (structural), `baseline` (functional + quality +
  perf), `static-release` (musl/static-link), `live-smoke` (real source
  health metric, advisory).
- 16 audited + enabled sources (5 RSS, 11 HTML-config), all fixture-backed.
  27 sources audited total in the registry.
- 65 acceptance cases (64 hard + 1 advisory), all pass.
- CI: `ci.yml` (fmt + clippy + test + xtask check + check-matrix + MSRV 1.96
  + cargo-deny), `release.yml` (quality gates → musl build → container smoke
  → release with provenance attestation), `live-smoke.yml` (scheduled,
  advisory).
- 13 ADRs, 14 plan documents, reference docs (config schema, CLI, output
  schema), runbook, acceptance-case documentation.

### Security

- No `unsafe` in any crate (`#![forbid(unsafe_code)]`).
- Uninstall deletes only known app-owned paths; never `rm -rf $HOME`; rejects
  symlinks in path components; refuses unmanaged/`cargo run` binaries without
  `--force-unmanaged`.
- Self-update: HTTPS only, fixed release repo, no auto `sudo`, no downgrade,
  checksum verification before replace, rollback on failure.
- Release workflow uses least-privilege permissions (workflow-level
  `contents: read`, per-job escalation only where needed) with
  `persist-credentials: false` on all checkouts (R3-P0-03).

### Known Limitations (v0.1)

- `sources check` is a deferred stub (ADR-0009); live source health is via
  `cargo xtask live-smoke` (advisory, scheduled).
- `doctor` is local-only (no `--network` flag).
- Dedup is O(n²) — accepted for v0.1 scale (P1-01 design decision).
- EventId = `BLAKE3(title+URL)` — a title change produces
  `event_cancelled + event_added` instead of `event_updated` (ADR-0008).
- Global candidate cap (10k) truncates by source-id order — accepted bias
  for v0.1 (P0-04(a) design decision).

[0.1.0]: https://github.com/Develata/math_talk_radar/releases/tag/v0.1.0
[Unreleased]: https://github.com/Develata/math_talk_radar/compare/v0.1.0...HEAD
