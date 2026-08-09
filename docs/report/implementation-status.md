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
