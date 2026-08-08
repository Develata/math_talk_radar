# AGENTS.md — `crates/radar-state`

## Purpose

Embedded local persistence: canonical event/talk/media fingerprints,
`first_seen` / `last_seen`, source-health history, change-detection state,
schema version. Backed by `redb` (ADR-0002) starting in M3.

## Authority

Follows `docs/plan/07_state_change_detection.md`.

## Hard boundaries

- **Persistence only.** No fetch, no ranking decisions, no parsing.
- **No `reqwest`, no `scraper`, no `feed-rs`, no `tokio`.** `redb` is the only
  storage engine.
- **Never store** full HTML, videos, cookies, or auth tokens (§22).
- **Migrations are transactional** (§65): a failure leaves no half-migrated
  state. Destructive migrations must be explicit.
- **`#![forbid(unsafe_code)]`.**

## Change detection (§23)

Must emit: `event_added`, `event_updated`, `schedule_added`, `speaker_added`,
`livestream_added`, `media_added`, `media_removed`, `event_cancelled`. The
canonical baseline: an event seen with `media=[]` then re-seen with a new
video must produce `media_added`.
