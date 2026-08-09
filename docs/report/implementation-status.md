# Implementation Status

> Evidence only — non-authoritative (§0.2). Updated per milestone.

## Current milestone: M1 — Core Domain

- [x] Date parser: `parse_date` (ISO, same-month range, cross-month range, US
      format, DMY single) + `interval_overlap`. 11 unit tests (DATE-001..005).
- [x] Normalization pipeline: `normalize_name` (NFC + case + whitespace) +
      `word_boundaries` (Unicode segmentation). 8 unit tests.
- [x] Scholar matcher: `match_scholars` with `MatchContext` role protection
      (§6.2). Ambiguous-surname filtering (Li/Wang/Tao/Yau/Gross/Wei/Wu). 7 unit
      tests (PER-001..003).
- [x] Topic matcher: `match_topics` with single-word word-boundary vs multi-word
      substring matching. 8 unit tests (TOP-001).
- [x] Ranking composer: `score_event` with 6 signal components (§26) +
      `InterestWeights`. 8 unit tests (RANK-001..003).
- [x] Golden datasets: 56 date cases, 66 people cases, 25 ranking cases (147
      total). §47 metrics: date accuracy 1.000, scholar precision 1.000, recall
      1.000, role-protection FP 0. `cargo test --test golden` passes.
- [x] M1 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, `cargo test --workspace`, `cargo xtask
      check`, `cargo xtask check-matrix` — all pass.
- [x] 12 acceptance cases flipped to `pass`: DATE-001..005, PER-001..003,
      TOP-001, RANK-001..003.
- [x] One multilingual alias added to `config/scholars.toml` (陶哲轩 →
      terence-tao) for PER-002.

## Next: M2 — Source Adapters & Fetching

RSS/ICS/JSON-LD/HTML adapters in `radar-adapters`, fetch coordinator in
`radar-fetch`, mock-server HTTP tests, talk extraction (TALK-001).

## M2 — Fetch & Adapters (complete)

- [x] ADR-0005: `HtmlSelectors` struct on `SourceSpec` for configured HTML
      adapter (§64 backward compatible, optional field).
- [x] Dependencies: MSRV 1.85→1.96 (feed-rs 2.4 / quick-xml 0.41 for
      RUSTSEC-2026-0194/0195), scraper 0.22→0.27, +icalendar/futures/wiremock.
      ADR-0001 updated.
- [x] Async fetch engine (`radar-fetch/src/engine.rs`): concurrency (global
      semaphore 8 + per-host semaphore 2), timeout (per-request min of
      request_timeout and remaining deadline), manual redirect loop with
      per-hop host allowlist (Policy::none), robots cache (OnceCell
      thundering-herd fix), request budget, failure isolation (JoinSet,
      is_panic()→ParseError, never abort). 8 Oracle architecture findings
      integrated. 7 unit tests.
- [x] Shared adapter helpers (`radar-adapters/src/helpers.rs`):
      extract_html_fields, detect_media (YouTube/Vimeo/Bilibili/PDF),
      classify_access (most-restrictive-wins), detect_event_type (all 14
      variants, specific-before-generic). 48 unit tests.
- [x] RSS adapter: feed-rs parsing, XXE-safe (quick-xml). 6 tests.
- [x] ICS adapter: icalendar with mandatory nesting-depth guard (>32 BEGIN:
      → Err(Parse), §67 DoS prevention). 11 tests.
- [x] JSON-LD adapter: schema.org Event extraction, TALK-001 performer→Talk
      with Speaker role. 8 tests.
- [x] Configured HTML adapter: CSS selectors via ADR-0005 HtmlSelectors. 8 tests.
- [x] Generic HTML fallback: keyword matching all 14 EventType variants,
      nav-stopword filter (≥90% precision). 6 tests.
- [x] http_mock tests (wiremock): SRC-006..008, HTTP-001..003, REL-002. 7 tests.
- [x] Reliability tests (wiremock): REL-001 fault isolation (30% failure, no
      abort), robots de-dup (OnceCell). 2 tests.
- [x] Adapter fixture tests: SRC-001..005, MED-001..003, TALK-001, §67 XXE +
      ICS depth guard + malformed ICS. 9 sanitized fixtures, 12 tests.
- [x] M2 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, `cargo test --workspace` (171 tests),
      `cargo xtask check`, `cargo xtask check-matrix` — all pass.
- [x] 17 acceptance cases flipped to `pass`: SRC-001..008, MED-001..003,
      HTTP-001..003, REL-001..002, TALK-001. Coverage matrix updated
      (TALK-001 M4→M2, REL-001..002 M8→M2).

## Next: M3 — State & Dedup

## M3 — State, Dedup & Change Detection (complete)

- [x] Dependencies: redb 2.6 added to `radar-state` (already a workspace dep),
      serde_json for event serialization, tempfile dev-dep for integration
      tests. §11 boundaries preserved (no reqwest/scraper/tokio in radar-state).
- [x] Conservative deterministic dedup algorithm (`radar-core/src/dedup.rs`):
      §25 signal priority CanonicalUrl → SourceCanonicalId →
      TitleDateOrganizer → TitleDateLocation. `are_duplicates(a, b, signal)`,
      `duplicate_signal(a, b)` (strongest match), `merge_events` (union
      sources/media/talks/people, earliest first_seen / latest last_seen),
      `dedup_events` (stable single-pass O(n²) cluster merge, pre-sorted by id
      so deterministic regardless of input order). 12 unit tests.
- [x] Schema additions (§64-compatible optional fields in v0.x, no
      schema_version bump): `Event.url: Option<Url>` (event's canonical
      detail-page URL, drives CanonicalUrl signal — was previously only on
      EventStub, lost on enrich), `SourceEvidence.native_id: Option<String>`
      (source-declared canonical event id, drives SourceCanonicalId signal),
      `EventDate::start_date()` public accessor. All 11 Event constructors and
      21 SourceEvidence constructors updated.
- [x] redb-backed repository (`radar-state/src/repository.rs`): `open` (create
      + migrate, read/write), `open_read_only` (STATE-004 no-write path),
      `store_event` (upsert, preserve existing first_seen_at, stamp
      last_seen_at = caller-supplied now — repository never reads a wall clock,
      §11 determinism), `get_event`, `list_events` (sorted-by-id iteration),
      `store_source_health`, `schema_version`.
- [x] Schema + migrations (`radar-state/src/schema.rs`, `migrations.rs`):
      3 tables (EVENTS, SOURCE_HEALTH, SCHEMA_VERSION), `STATE_SCHEMA_VERSION
      = 1`. `run_migrations`: single write txn creates all v1 tables (no-op if
      present), writes schema-version row if absent, rejects unsupported
      versions. Transactional §65 — a failure rolls back cleanly.
- [x] Change detection (`radar-state/src/changes.rs`): `detect_changes(previous,
      current, now)` emits EventAdded, EventCancelled, MediaAdded,
      LivestreamAdded (more-specific form of MediaAdded), MediaRemoved. Canonical
      baseline §23: event first seen with media=[] then re-seen with a new
      video → MediaAdded. Unchanged events emit nothing (STATE-002). Output
      sorted by (event_id, kind) for §11 determinism. 6 unit tests.
- [x] Dedup golden tests: 36 pairs (18 positive DEDUP-001, 18 negative
      DEDUP-002) across all four signals. §47 asserts: precision = 100%
      (false_positives == 0, wrong merge = release blocker), recall ≥ 90%, ≥30
      pairs. REL-003: `deterministic_id` stable across calls, distinct for
      distinct inputs, BLAKE3-prefixed.
- [x] State integration tests: STATE-001 (first_seen persisted across
      close/reopen), STATE-002 (unchanged → no records), STATE-003 (canonical
      media_added baseline, first_seen preserved on re-store), STATE-004
      (read-only repository rejects writes, file size unchanged). 4 tests
      against a real redb database in tempfile TempDir.
- [x] M3 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, `cargo test --workspace` (194 tests),
      `cargo xtask check`, `cargo xtask check-matrix` — all pass.
- [x] 7 acceptance cases flipped to `pass`: DEDUP-001, DEDUP-002, REL-003,
      STATE-001..004. Coverage matrix M3 row unchanged (already listed these
      cases). Total 36 pass / 29 pending.

## Next: M4 — CLI Composition

## M4 — CLI Composition (complete)

- [x] Config model: `SourcesConfig` wrapper in `radar-core/src/config.rs` with
      `parse()`, `embedded()` (include_str! of `config/sources.toml`), `enabled()`
      filter (CFG-001). Moved `toml` from radar-core dev-deps to main deps.
- [x] CLI config loader (`apps/cli/src/config_loader.rs`): `load_sources(path)`
      reads TOML from `--sources` override or falls back to embedded default.
      Missing/unparseable file fails closed (CFG-002, exit 3).
- [x] Scan pipeline (`apps/cli/src/scan_engine.rs`): load sources → filter
      enabled → build `FetchClient` with default `HttpPolicy` → `fetch_all` with
      `default_adapter` factory → extract events from `EventCandidate`s →
      `dedup_events` → `score_event` with source-tier map from config →
      truncate to `--max-events` → build `ScanOutput` envelope. Zero enabled
      sources → exit 4 (HTTP-005).
- [x] Exit-code constructors on `CliError`: `config` (3), `zero_sources` (4),
      `state` (5), `serialization` (6).
- [x] Command wiring: `scan` (run_scan + render → stdout JSON), `sources list`
      (human-readable table), `schema` (ScanOutput skeleton as JSON), `doctor`
      (binary/config-dir/state-dir/schema-version, XDG-aware). `update`/`uninstall`
      stay stubs (M5); `sources check` stays not_implemented (M6).
- [x] Integration tests (`apps/cli/tests/integration.rs`): 7 `assert_cmd` tests
      driving the real binary, `wiremock` for HTTP-004/005:
      CLI-001 (--help lists 6 subcommands), CLI-002 (--version exit 0 no
      network), CLI-003 (scan stdout pure JSON), CLI-004 (stderr/stdout
      separated), CFG-002 (invalid config exit 3), HTTP-004 (partial source
      failure exit 0), HTTP-005 (zero usable sources exit 4).
- [x] M4 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, `cargo test --workspace` (204 tests),
      `cargo xtask check`, `cargo xtask check-matrix` — all pass.
- [x] 8 acceptance cases flipped to `pass`: CLI-001..004, CFG-001..002,
      HTTP-004..005. Total 44 pass / 21 pending.

## Next: M5 — Self-update & Uninstall

Self-update (SHA-256 verify + rollback copy) and uninstall (known-path deletion
only) per `docs/plan/10_update_uninstall.md`.

## M5 — Self-update & Uninstall (complete)

- [x] Lifecycle infrastructure (`apps/cli/src/lifecycle/`):
      `paths.rs` (known app-owned path resolution: binary via manifest or
      `current_exe`, config/cache/data via XDG env vars, `safe_canonicalize`
      rejecting empty/`/`/`$HOME`, `is_unmanaged_binary` detecting `target/`,
      `MATH_TALK_RADAR_RELEASE_API` env override for tests), `manifest.rs`
      (`InstallManifest` saved to data dir; load returns None for missing/
      unparseable → unmanaged), `update.rs`/`uninstall.rs` implementations.
- [x] `CliError::update` (exit 10) and `CliError::uninstall` (exit 11) per §32.
- [x] Self-update (`lifecycle/update.rs`, §34.2): `check()` fetches
      `/releases/latest`, SemVer compares, writes nothing (UPD-001). `run()`
      downloads binary + `.sha256`, verifies SHA-256 (UPD-002 fail=preserve,
      exit 10), self-tests candidate (UPD-004 fail=cleanup+preserve), creates
      rollback copy, atomic `rename` replace, self-tests replaced (fail=restore
      rollback), cleans up, updates manifest.
- [x] Uninstall (`lifecycle/uninstall.rs`, §35): `--dry-run` prints plan,
      mutates nothing (UNS-001). `--keep-data --yes` deletes binary+config+
      cache+manifest+temp, preserves data (UNS-002). `--purge --yes` deletes
      everything (UNS-003). Unmanaged binary (no manifest, under `target/`)
      protected without `--force-unmanaged` (UNS-004). Requires `--keep-data`
      or `--purge`; noninteractive requires `--yes`. `safe_canonicalize`
      rejects empty/`/`/`$HOME`; Rust fs APIs only, no `rm -rf`.
- [x] Lifecycle sandbox tests (`apps/cli/tests/lifecycle_sandbox.rs`): 8
      `assert_cmd` tests in temp sandboxes (real install never touched).
      UPD tests use `wiremock` for the fake release API via
      `MATH_TALK_RADAR_RELEASE_API`; UNS tests set `XDG_*_HOME` to temp dirs.
- [x] M5 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, `cargo test --workspace` (212 tests),
      `cargo xtask check`, `cargo xtask check-matrix` — all pass.
- [x] 8 acceptance cases flipped to `pass`: UPD-001..004, UNS-001..004.
      Total 52 pass / 13 pending.

## Next: M6 — Live Source Audit

Audit ≥ 20 sources, enable ≥ 10 with fixtures + golden + live smoke per
`docs/plan/` source-registry sections. Gate: LIVE-001/002.

## M6 — Live Source Audit (complete)

- [x] xtask LIVE-001/002 count validation in `validate_source_registry`
      (`xtask/src/main.rs`): counts `audited`+`enabled` rows (LIVE-001 ≥ 20),
      `enabled = true` + non-empty `fixture` rows (LIVE-002 ≥ 10), distinct
      adapter kinds among enabled (≥ 3). Fires only when `pending_audit_count
      == 0` so the gate doesn't fail during an in-progress audit.
- [x] 24 sources audited via direct HTTP probing. Findings recorded in
      `docs/registry/source-registry.tsv`:
      - 15 enabled + fixture-backed (2 RSS, 1 JSON-LD, 12 HTML-config).
      - 5 dynamic_unsupported (slmath, ias-math, birs, oberwolfach, scgp).
      - 1 broken (mathmeetings — DNS failure).
      - 3 audited-no-fixture (harvard-math, berkeley-math, ems-calendar).
      - 0 pending_audit rows remain.
- [x] 15 sanitized fixtures under `crates/radar-adapters/tests/fixtures/sites/`
      (scripts/styles/SVGs stripped, JSON-LD blocks preserved for stanford-math
      and cirm).
- [x] Golden tests (`crates/radar-adapters/tests/site_audits.rs`): 8 tests
      covering all 3 adapter kinds — RSS (clay, ihes), JSON-LD (stanford-math),
      HTML-config (fields, eth-math, mpim, oxford, princeton). HTML-config
      uses permissive `list: "body", list_link: "a"` selectors that yield ≥ 1
      stub from each fixture; site-specific selector tuning is post-M6 (§P-5).
- [x] `config/sources.toml` promoted from empty M0 skeleton to 15 enabled
      entries with real entrypoints, allowed_hosts, adapter kinds, and fixture
      paths. Tier values lowercase (`s`/`a`/`b`) to match serde
      `rename_all = "snake_case"` on `SourceTier`. HTML-config sources carry
      `[sources.selectors]` blocks with body→a link extraction.
- [x] M6 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, `cargo test --workspace` (220 tests),
      `cargo xtask check`, `cargo xtask check-matrix` — all pass.
- [x] 2 acceptance cases flipped to `pass`: LIVE-001, LIVE-002. Total 54 pass
      / 15 pending.

## Next: M7 — Performance & Release

Perf baseline (PERF-001/002), supply chain (SEC-002), static musl release
(RELS-001..003), live-smoke workflow (LIVE-003, advisory).
