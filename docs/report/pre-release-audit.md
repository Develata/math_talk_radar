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
