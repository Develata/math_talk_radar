# Changelog

All notable changes to `math_talk_radar` are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [0.1.0] — 2026-08-23

First public release, rewritten before the v0.1.0 tag/Release is finalized.
Pure Rust CLI for discovering public mathematics conferences, talks, lecture
series, recordings, slides, and related resources. No LLM, no browser
automation, no JS runtime.

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
  HTML-generic (fallback). Structured adapters resolve relative links against
  the post-redirect `final_url`.
- RSS/Atom native entry IDs are preserved; feed publication timestamps feed
  media metadata while detail-page event dates remain authoritative. Feed
  authors are not implicitly promoted to mathematical speakers.
- ICS retains legal URL-less VEVENTs via stable synthetic URLs, preserves UID
  as native identity, resolves relative URL values, handles inclusive DTEND /
  positive DURATION safely, and rejects overflow/inverted ranges without
  panicking.
- JSON-LD resolves relative `url`/path-form `@id`, preserves raw `@id` as
  native identity, avoids fragment-ID EventId collisions, and imports valid
  `endDate` ranges. Ambiguous same-name nodes cannot override structured
  identity matches.
- Talk-level program evidence participates in ranking: talk title/abstract
  topics roll up to a deduplicated event topic set; structured talk speakers
  receive scholar-registry tags; talk media/speakers participate through the
  existing max-capped media/people signals without double counting.
- Fetch client with rustls TLS, HTTP policy, retry, robots (RFC 9309),
  per-source + global request budgets, global scan deadline.
- State schema v4 with change detection: event added/updated/cancelled,
  tombstone retention, source-health history, collision-safe change-log keys,
  and fixed-width ordered timestamps (ADR-0011, ADR-0013). Transactional
  v1→v2→v3→v4 migration fails closed on malformed rows or migration key
  collisions.
- CLI surface (§27): `scan`, `sources list`, `doctor`, `update`, `uninstall`,
  `schema`. Interactive TTY uninstall prompt (§35.1) with non-TTY refusal
  (§35.2). `--dry-run` is zero-mutation.
- Self-update: SHA-256 verification, rollback copy, download timeout + size
  caps, per-hop URL validation, symlink-rejection at rollback path, update lock
  shared with uninstall, and pre-side-effect rejection on unsupported targets.
- Output schema `1.0` (§64): `schemars`-derived JSON Schema with a golden-file
  drift test and an immutable v1.0 backward-compatibility gate.
- Source config semantic validation rejects duplicate/empty IDs, invalid
  enabled entrypoints, zero budgets/depths, missing HTML selectors, and
  unsupported media strategies. v0.1 supports only RSS `youtube_channel`.
- xtask validators: `check`, `check-matrix`, `static-release`, `live-smoke`,
  and a full `baseline` covering functional/quality gates, RSS peak memory,
  1k/5k/10k distinct-event pipelines, 10k high-collision dedup, redb
  write/reopen, JSON/JSONL streaming, release binary size, and CLI startup.
- 16 audited + enabled sources (5 RSS, 11 HTML-config), all fixture-backed.
  27 sources audited total in the registry.
- 65 acceptance cases (64 hard + 1 advisory), all pass.
- CI: fmt, clippy, workspace tests, xtask acceptance checks, `forbid(unsafe)`,
  cargo-deny, radar-core/workspace coverage gates, full synthetic baseline, and
  declared MSRV 1.96. Tag release additionally builds
  `x86_64-unknown-linux-musl`, runs the authoritative static-release check,
  verifies checksum in clean Ubuntu 22.04, then creates the Release with
  provenance attestation.
- 13 ADRs, 14 plan documents, reference docs (config schema, CLI, output
  schema), runbook, acceptance-case documentation.

### Security

- No `unsafe` in any crate (`#![forbid(unsafe_code)]`).
- Uninstall deletes only known app-owned paths; never `rm -rf $HOME`; rejects
  symlinks in path components; refuses unmanaged/`cargo run` binaries without
  `--force-unmanaged`.
- Self-update: HTTPS only, fixed release repo, no auto `sudo`, no downgrade,
  checksum verification before replace, rollback on failure, redirect-hop host
  validation.
- Release workflow uses least-privilege permissions (workflow-level
  `contents: read`, per-job escalation only where needed) with
  `persist-credentials: false` on checkouts.
- cargo-deny is pinned to 0.20.2; GitHub Actions are pinned by commit SHA.

### Release assets

- Prebuilt binary: **`x86_64-unknown-linux-musl` only**.
- Binary is accompanied by a `.sha256` file and GitHub build-provenance
  attestation. Checksums are generated from the rewritten release artifact and
  must not be reused from the earlier v0.1.0 publication.

### Known Limitations (v0.1)

- `sources check` is a deferred stub (ADR-0009); live source health is via
  `cargo xtask live-smoke` (advisory, scheduled).
- `doctor` is local-only (no `--network` flag).
- Dedup retains the conservative greedy O(n²) semantics; the full 10k baseline
  records its actual release-scale cost rather than changing cluster choice.
- EventId = `BLAKE3(title+URL)` — a title change produces
  `event_cancelled + event_added` instead of `event_updated` (ADR-0008).
- Global candidate cap (10k) truncates by source-id order — accepted bias for
  v0.1; affected sources are marked partial so truncation cannot authorize
  destructive prune.
- ICS DTSTART/TZID is represented at date precision in v0.1; importing the
  complete timezone-aware VEVENT payload would require a broader adapter/domain
  transport change and is deferred rather than approximated silently.

[0.1.0]: https://github.com/Develata/math_talk_radar/releases/tag/v0.1.0
[Unreleased]: https://github.com/Develata/math_talk_radar/compare/v0.1.0...HEAD
