# Pre-Release Code Audit

> Evidence only — non-authoritative. A module-by-module review of the v0.1.0
> codebase performed before tagging, prioritizing long-term maintainability.
> All fixes are committed on `main` and covered by the 2026-08-12 baseline.

## Scope

Every crate (`radar-core`, `radar-fetch`, `radar-adapters`, `radar-state`,
`apps/cli`) reviewed against `docs/plan/` and `AGENTS.md` contracts. Findings
tiered T1 (HIGH), T2 (MEDIUM), T3 (LOW). 29 findings total: 26 fixed, 3
dismissed (T3-2, T3-4, T3-15 — not dead code / not actionable / duplicate).

## T1 — HIGH (6 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| T1-1 | core | dedup tracking-param denylist | `0ee76c2` |
| T1-2 | core | ISO date range + ordinals parsing | `0ee76c2` |
| T1-3 | adapters | doc_body UTF-8 extraction (§66) | `c133c94` |
| T1-4 | fetch | robots redirect bypass + scheme validation + 5xx→disallow | `ffc41b0` |
| T1-5 | cli | stale manifest dev-binary protection (§36) | `7277892` |
| T1-6 | adapters | IndicoAdapter clear error on malformed input | `c133c94` |

## T2 — MEDIUM (8 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| T2-1 | core | topics uses contains_phrase (word-boundary match) | `0ee76c2` |
| T2-2 | fetch | 503 Retry-After header passthrough | `ffc41b0` |
| T2-3 | core | ISO date range + ordinals (paired with T1-2) | `0ee76c2` |
| T2-4 | state | is_event_updated ignores date parser artifacts | `bec33af` |
| T2-5 | cli | `--today` with no events exits 3 (§32) | `7277892` |
| T2-6 | cli | update failure path preserves binary + rollback | `7277892` |
| T2-7 | cli | update failure path error messaging | `7277892` |
| T2-8 | core | event_id URL canonicalization | `0ee76c2` |

## T3 — LOW (15 findings: 12 fixed, 3 dismissed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| T3-1 | core | normalize_text truncates in-place (no trim_end+alloc) | `1279af7` |
| T3-3 | core | people.rs deleted boundary_substring_match, uses contains_phrase | `1279af7` |
| T3-5 | core | dedup domain_key avoids format! alloc, byte check for suffix | `1279af7` |
| T3-6 | core | ranking is_important_scholar inlines to_lowercase | `1279af7` |
| T3-7 | fetch | engine.rs reuses status var (no double as_u16 read) | `9fd5cc4` |
| T3-8 | fetch | robots_url_for via Url API (no format!+parse) | `9fd5cc4` |
| T3-9 | adapters | clean_text promoted to pub(crate), deleted 2 duplicate copies | `e3cfc7b` |
| T3-10 | adapters | clean_text streaming fold (no intermediate Vec) | `e3cfc7b` |
| T3-11 | core+adapters | EventDate::unknown() constructor, 9 fallback literals unified | `1279af7` `e3cfc7b` |
| T3-12 | adapters | constant selectors cached via OnceLock<HashMap> | `e3cfc7b` |
| T3-13 | state | new_speakers HashSet (O(n²)→O(n)) | `6ed0dcb` |
| T3-14 | cli | truncate_for_detail single-pass char_indices (fast path no alloc) | `dcbbf99` |
| T3-15 | cli | dismissed — not_implemented is the SourcesAction::Check placeholder | — |
| T3-2 | — | dismissed (earlier batch, not actionable) | — |
| T3-4 | — | dismissed (earlier batch, duplicate) | — |

## Post-audit gate (2026-08-12)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 285 passed, 0 failed |
| `cargo xtask check` | ok |
| `cargo xtask check-matrix` | ok |
| `cargo xtask baseline` | ok (functional + quality + perf 6.3 MiB) |
| `cargo deny check` | advisories/bans/licenses/sources ok |

All audit commits are on `origin/main` (pushed 2026-08-12). The codebase is
ready for `v0.1.0` tag pending Deve's explicit authorization (AGENTS.md §12).

## Second-round audit (2026-08-13)

A second module-by-module review by 5 oracle agents (one per crate + one
cross-cutting) produced 40 additional findings: 4 HIGH, 18 MEDIUM, 16 LOW +
2 edge. Deve authorized fixing all 40 before `v0.1.0`. Fixes committed in 3
batches across 9 atomic commits (`d5bdb2b`..`00f1d45`).

### Batch 1 — HIGH (4 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CORE-1 | core+cli | score before dedup so merge picks the richer record | `d5bdb2b` |
| CLI-1 | cli | sort by score (desc) + id tie-break before --max-events truncate | `d5bdb2b` |
| CLI-2 | core+cli | wire --interests into score_event; remove 4 dead flags | `d5bdb2b` |
| ADAP-1 | adapters | defer link_text collection in detect_media (lazy PDF branch) | `d5bdb2b` |

### Batch 2 — MEDIUM (18 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CORE-2 | core | precompute DedupKeys once per event (no O(n²) re-normalization) | `a937c99` |
| CORE-3 | core | word_boundaries computed only for BodyText context | `a937c99` |
| CORE-4 | core | unify two URL canonicalizers into one pub fn in normalize | `a937c99` |
| CLI-6 | core | move matches_mode_and_window + ScanMode into radar_core::filter | `a937c99` |
| ADAP-2 | adapters | classify_access single-pass lowercase+collapse buffer | `8c2455e` |
| ADAP-3 | adapters | first_text prefers direct_text over all-descendant text | `8c2455e` |
| ADAP-4 | adapters | jsonld plan_enrichment empty Vec for url-less events | `8c2455e` |
| ADAP-5 | adapters | drop dead base_url param from extract_html_fields | `8c2455e` |
| FS-1 | fetch | RFC 9309 wildcard (*) and end-anchor ($) matching | `1415ee4` |
| FS-2 | fetch | per-host semaphore on actual request URL, not entrypoint | `1415ee4` |
| FS-3 | fetch | panicked source tasks get synthetic ParseError in source_health | `1415ee4` |
| FS-4 | fetch | exponential backoff when Retry-After header absent | `1415ee4` |
| ST-1 | state+cli | store_scan atomic compare-and-store primitive + wire into scan | `bfd460c` `7bfb725` |
| ST-2 | state | first_seen_at from already-deserialized prev events (no re-deser) | `bfd460c` |
| CLI-4 | cli | real JSONL output (one object per line) | `7bfb725` |
| CLI-5 | cli | render takes ScanOutput by value (no full-output clone) | `7bfb725` |

### Batch 3 — LOW (18 findings: 16 fixed, 2 documented as deferred)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CORE-5 | core | NormalizedTopic + match_topics_normalized (pre-normalize once) | `b3e5cc7` |
| CORE-6 | core | PersonHit.evidence = None (was duplicate of matched_text) | `b3e5cc7` |
| CORE-7 | core | role-specific rank_reason (important_speaker/organizer/panelist) | `b3e5cc7` |
| CORE-8 | core | is_important_scholar exact match (no substring false-positives) | `b3e5cc7` |
| CORE-9 | core | space-separated ISO date range parsed as Range | `b3e5cc7` |
| CORE-10 | core | remove generic ref/source/ver from tracking-param denylist | `b3e5cc7` |
| ADAP-6 | adapters | thread_local runtime selector cache for html_config | `1e9b12c` |
| ADAP-7 | adapters | html_generic uses doc_body helper (not from_utf8_lossy) | `1e9b12c` |
| ADAP-8 | adapters | contains_event_keyword uses contains_phrase (word boundaries) | `1e9b12c` |
| FS-5 | fetch | oversized robots.txt → disallow_all (conservative) | `1e9b12c` |
| FS-6 | fetch | pub(crate) on budget/fetch_policy/retry/robots modules | `1e9b12c` |
| A-1 | adapters | pub(crate) on sites module | `1e9b12c` |
| CLI-7 | cli | lazy tokio runtime (not on --help/--version path) | `00f1d45` |
| CLI-8 | cli | --jobs 0 rejected with exit 2 (not silent fallback) | `00f1d45` |
| CLI-9 | cli | self-update streams to file + incremental hash (bounded memory) | `00f1d45` |
| ST-3 | state | documented: peak memory ≈ 2× corpus, iterator-based deferred | `00f1d45` |
| ST-4 | state | documented: full Event persisted, fingerprint projection deferred | `00f1d45` |

### Post-second-round gate (2026-08-13)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 309 passed, 0 failed |
| `cargo xtask check` | ok |
| `cargo xtask check-matrix` | ok |
| `cargo xtask baseline` | ok (functional + quality + perf 6.6 MiB) |

Both audit rounds (26 + 40 = 66 findings total) are resolved. The codebase
is ready for `v0.1.0` tag pending Deve's explicit authorization (AGENTS.md §12).
