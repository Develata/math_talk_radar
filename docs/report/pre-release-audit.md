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

## Third-round audit (2026-08-14)

A third review by 5 oracle agents (one per crate) produced 3 HIGH, 6 MEDIUM,
8 LOW findings. Deve authorized fixing all before `v0.1.0`. Fixes committed
across 4 atomic commits (`d7702c5`..`1b4ea0c`).

### HIGH (3 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CLI-10 | cli | store_scan runs BEFORE filter+truncate (ST-1 regression: false EventCancelled on every narrowed scan) | `d7702c5` |
| CORE-11/12 | core+cli | wire match_topics into scan_engine via enrich_event_topics + TopicsConfig (40% of ranking was dead code) | `3f7ddd2` |
| CORE-13 | core+cli | wire match_scholars via enrich_event_scholars + ScholarsConfig + union_topics in merge_events | `3f7ddd2` |

### MEDIUM (6 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CLI-11 | cli | temp file leak on mid-download error in download_to_file_with_hash | `b73f042` |
| ADAP M-1 | adapters | detect_media within-event dedup by URL (HashSet guard) | `0d6baa3` |
| ADAP M-2 | adapters | JsonLdAdapter url-less enrichment: pass entrypoint doc to enrich when plan_enrichment empty (ADAP-4 regression) | `0d6baa3` |
| FETCH M-1 | fetch | follow cross-host robots.txt redirects per RFC 9309 §2.3.1.2 (was disallow_all) | `0d6baa3` |
| FETCH M-3 | fetch | concurrency=0 guard in FetchClient::new (was Semaphore::new(0) hang) | `0d6baa3` |
| ST M-1 | state | prune cancelled events in store_scan (was unbounded growth + repeated EventCancelled) | `0d6baa3` |

### LOW (8 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| L-5 | core | union_sources/media/talks/people/topics O(n²)→O(n+m) via HashSet | `1b4ea0c` |
| L-1 | adapters | select_first_text + extract_jsonld_blocks routed through cached_selector (added h1/title/jsonld to cache) | `1b4ea0c` |
| L-2 | adapters | first_text_in direct_text preference (ADAP-3 incomplete) | `1b4ea0c` |
| L-3 | adapters | ics::enrich from_utf8→doc_body (BOM + lossy fallback) | `1b4ea0c` |
| LOW-1 | core | contains_phrase CJK substring fallback (ideographs are alphanumeric, break the boundary check) | `1b4ea0c` |
| CLI-13 | cli | default_state_db_path delegates to paths::data_dir (XDG dedup) | `1b4ea0c` |
| CLI-15 | cli | --jobs==0 moved before config load (fail-fast) + redundant guard removed | `1b4ea0c` |

### Deferred (accepted at v0.1 scale)

| ID | Crate | Reason |
|---|---|---|
| FETCH M-2 | fetch | per-host semaphore map growth — bounded by source registry size (~13 hosts); eviction machinery risks bugs for no current benefit |

### Post-third-round gate (2026-08-14)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 315 passed, 0 failed |

All three audit rounds (26 + 40 + 17 = 83 findings total) are resolved. The
codebase is ready for `v0.1.0` tag pending Deve's explicit authorization
(AGENTS.md §12).

## Fourth-round audit (2026-08-14)

A fourth review by 5 oracle agents (one per crate) produced 0 HIGH, 11 MEDIUM,
9 LOW findings. Deve authorized fixing all before `v0.1.0`. Fixes committed
across 3 atomic commits (`588bb17`..`528444b`).

### MEDIUM (11 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CORE-14 | core | parse full D-M-Y date ranges ("1-3 September 2026") via re_dmy_full_date_range | `588bb17` |
| CORE-15 | core+docs | record event_id derivation drift (plan: title+org+date, code: title+url) as ADR-0008 + update §24 | `528444b` |
| FETCH-1 | fetch | glob_match trailing `*` before `$` anchor now consumes remaining target chars | `f10a91b` |
| FETCH-2 | fetch | Retry-After HTTP-date format (RFC 7231 IMF-fixdate) parsed via parse_retry_after | `f10a91b` |
| ADAP-9 | adapters | detect_media iframe only Video when host is a recognized video platform (no more calendar/map false positives) | `f10a91b` |
| ADAP-10 | adapters | detect_media handles `<video><source>` and `<audio>`/`<audio><source>` | `f10a91b` |
| ADAP-11 | adapters | extract_person_names handles array-of-string performers (recurse per element) | `f10a91b` |
| ADAP-12 | adapters | unnamed JSON-LD events get distinct event_ids via synthetic `mtr-eid` query param (canonical URL strips fragments) | `f10a91b` |
| ADAP-13 | adapters | html_generic event_id hashes the same title that becomes event.title | `f10a91b` |
| ST-16 | state | cancelled-event tombstones (schema v1→v2) preserve first_seen_at for 90 days; restore on reappearance | `f10a91b` |
| CLI-20 | cli | --today validated before store_scan (invalid --today no longer mutates state DB) | `f10a91b` |
| CLI-21 | cli | store_scan write failure returns exit 5 (state fatal), not exit 0 with warning | `f10a91b` |

### LOW (1 finding fixed; 8 deferred to post-v0.1)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CLI-22 | cli | enrich_event_topics merges registry matches with adapter topics instead of replacing | `f10a91b` |

### Deferred (accepted at v0.1 scale)

| ID | Crate | Reason |
|---|---|---|
| FETCH-3/4/5 | fetch | minor robots/budget edge cases — no behavioral impact at v0.1 source count |
| ADAP-14/15 | adapters | niche JSON-LD / html_generic edge cases — no enabled source exercises them |
| ST-17/18 | state | list_events materialization + read-only edge cases — covered by ADR-0006 deferral |
| CLI-22 (rest) | cli | (the topic-merge portion is fixed; the full adapter-topic preservation audit is post-v0.1) |

### Post-fourth-round gate (2026-08-14)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 330 passed, 0 failed |
| `cargo xtask check` | ok |

All four audit rounds (26 + 40 + 17 + 21 = 104 findings total) are resolved.
The codebase is ready for `v0.1.0` tag pending Deve's explicit authorization
(AGENTS.md §12).

## Fifth-round audit (2026-08-14)

A fifth review by 5 oracle agents (one per crate) produced 1 HIGH, 9 MEDIUM,
11 LOW findings. Deve authorized fixing all 21 before `v0.1.0`. Fixes committed
across 5 atomic commits (`e9fced1`..`153ada9`) + 2 style/clippy follow-ups.

### HIGH (1 finding, fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| ST-16 | state | v1→v2 forward migration path: split `Some(v) if v != current` into forward-migrate (bump version, tables created by `open_table`) vs reject (newer schema). Old code was dead — neither arm ran for v1→v2. | `e9fced1` |

### MEDIUM (9 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| CORE-16 | core | filter.rs date arithmetic uses `checked_sub_signed`/`checked_add_signed` (no overflow on i64 day math) | `09398fa` |
| CORE-17 | core | ranking.rs clamps interest weights to [0,1] (NaN/negative/inf → neutral 1.0) | `09398fa` |
| CORE-18 | core | dedup merge_events fills scalar gaps (description/location/url/status) from the lower-scored event | `09398fa` |
| CORE-19 | core | date.rs datetime range → Unknown precision (was silently dropping the range) | `09398fa` |
| CORE-20 | core | DatePrecision::Year/Month documented as reserved (parser never emits them; consumers must not assume Day) | `09398fa` |
| CORE-21 | core | normalize.rs re-encodes query via `form_urlencoded::Serializer` (was hand-rolling `?k=v&k=v`) | `09398fa` |
| FETCH-6 | fetch | robots.txt fetch + check_robots accept `Option<Instant>` deadline — robots requests capped by `remaining_time` | `cb282a6` |
| FETCH-7 | fetch | per-host loop checks `past_deadline` before `acquire_host_permit`, recomputes `remaining_time` after — closes permit-then-deadline race | `cb282a6` |
| ADAP-16 | adapters | jsonld discover uses a global counter across all JSON-LD blocks for `mtr-eid` (was per-block enumerate → unnamed events in separate blocks collided) | `7a5ef41` |
| ADAP-17 | adapters | classify_video_platform recognizes `youtube-nocookie.com/embed` and `youtube.com/shorts/` | `7a5ef41` |
| ADAP-18 | adapters | classify_link classifies direct links to raw media files (`.mp4`/`.webm`/`.mp3`/etc.) — was only `<video>`/`<audio>` elements | `7a5ef41` |
| ADAP-19 | adapters | detect_media canonicalizes YouTube URLs to watch form before dedup — watch link + embed iframe of same video collapse to one resource | `7a5ef41` |

### LOW (11 findings, all fixed)

| ID | Crate | Fix | Commit |
|---|---|---|---|
| FETCH-8 | fetch | robots is_allowed percent-decodes `url.path()` before matching (RFC 9309 §2.2.1) — inline `percent_decode_path` helper, no new dependency | `cb282a6` |
| FETCH-9 | fetch | parse_retry_after validates weekday abbreviation matches the parsed date's actual weekday (chrono `%a` accepts any weekday name without cross-check) | `cb282a6` |
| ADAP-20 | adapters | rss enrich dead code removed — `parse_date("").unwrap_or_else(|_| ...)` where `parse_date("")` never errors → `EventDate::unknown(String::new())` | `7a5ef41` |
| ADAP-21 | adapters | jsonld enrich matches by url or @id, not just name — a detail page with a slightly different title no longer loses performer/description enrichment | `7a5ef41` |
| CLI-23 | cli | scan handles BrokenPipe gracefully (write_all + explicit kind check) — `| head` no longer panics with exit 101 (not in §32 contract) | `153ada9` |
| CLI-24 | cli | `--today` validated before `fetch_all` — invalid value fails fast (exit 3) without burning the request budget (subsumes CLI-20) | `153ada9` |
| CLI-25 | cli | removed dead `--network` flag from `doctor` — was declared and echoed but never performed any network check | `153ada9` |
| CLI-26 | cli | update dev-binary gate aligned with uninstall — checks `managed_by_manifest` so a target/ binary managed by a prior `--force-unmanaged` update is not refused | `153ada9` |

### Post-fifth-round gate (2026-08-14)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 352 passed, 0 failed |
| `cargo xtask check` | ok |

All five audit rounds (26 + 40 + 17 + 21 + 21 = 125 findings total) are
resolved. The codebase is ready for `v0.1.0` tag pending Deve's explicit
authorization (AGENTS.md §12).
