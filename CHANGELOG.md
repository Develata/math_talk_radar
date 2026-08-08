# Changelog

All notable changes to `math_talk_radar` are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — M0 (Repository Bootstrap)

- Rust 2024 workspace: `radar-core`, `radar-fetch`, `radar-adapters`,
  `radar-state`, `apps/cli`, `xtask`.
- Strict crate DAG enforced at compile time (§11); `#![forbid(unsafe_code)]` in
  every crate.
- Domain model: `Event`, `Talk`, `MediaResource`, `PersonHit`, `SourceSpec`,
  `FetchedDocument`, `FetchPlan`, `EventStub`, `EventCandidate`.
- `SourceAdapter` trait + M0 adapter stubs (rss, ics, jsonld, indico,
  html_config, html_generic).
- Fetch client with rustls TLS, HTTP policy, retry, robots, budget.
- State schema v1 with change-detection primitives.
- CLI surface (§27): `scan`, `sources`, `doctor`, `update`, `uninstall`,
  `schema`.
- xtask validators: `check` (source-registry + acceptance-matrix), `check-matrix`
  (structural + doc-coverage).
- 14 plan skeletons encoding the engineering contract (§1–§76).
- Acceptance matrix: 69 cases (68 hard, 1 advisory).
- Source registry: 24 audit candidates (all `pending_audit`, `enabled=false`).
- 4 ADRs; 4 reference docs; roadmap; runbook; CI skeletons.

[Unreleased]: https://github.com/Develata/math_talk_radar/compare
