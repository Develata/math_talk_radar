# AGENTS.md — `crates/radar-core`

## Purpose

Canonical domain model and the pure, deterministic algorithms that operate on
already-fetched data: normalization, date parsing, people/topic matching,
deduplication, ranking.

## Authority

This crate follows `docs/plan/02_domain_model.md` and
`docs/plan/06_normalization_matching.md`. Those plans are the truth source; code
here is their projection.

## Hard boundaries

- **No I/O.** No `reqwest`, no `redb`, no `scraper`, no `tokio`, no filesystem.
- **No parsing of external documents.** Adapters parse HTML/feeds; core receives
  already-extracted fields.
- **Deterministic.** Identical inputs produce identical IDs, scores, and dedup
  decisions. No clocks, no randomness, no network-derived ordering.
- **`#![forbid(unsafe_code)]`.** No `unsafe` ever, anywhere in this crate.

## What belongs here

- `Event` / `Talk` / `MediaResource` / `PersonHit` / `TopicMatch` / `EventDate`.
- Identity hashing (`blake3` over normalized identity fields, §24).
- Date parsing, normalization, scholar/topic matching, dedup, ranking — all pure.

## What does NOT belong here

- Fetching, retry, concurrency, robots policy → `radar-fetch`.
- HTML/feed/ICS/JSON-LD parsing → `radar-adapters`.
- Persistence, migrations, change detection state → `radar-state`.
- CLI composition, output rendering → `apps/cli`.
