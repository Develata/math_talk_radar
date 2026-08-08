# AGENTS.md — `crates/radar-adapters`

## Purpose

Turn already-fetched documents (`radar_core::FetchedDocument`) into
`EventStub` / `EventCandidate` via the `SourceAdapter` trait (§13). Adapters
parse structured sources first (RSS/Atom, ICS, JSON-LD, Indico), then
site-specific HTML, then a generic HTML fallback (§P-5).

## Authority

Follows `docs/plan/04_source_adapter_contract.md`.

## Hard boundaries

- **Pure document parsing.** No network. A parser must never call `reqwest` or
  any HTTP client. All I/O is done by the coordinator + `radar-fetch`.
- **No `reqwest`, no `redb`, no `tokio`.** Depends only on `radar-core` (+ parsing
  crates added in M2: `scraper`, `feed-rs`, `serde_json`).
- **Parser cannot panic** on untrusted input (§66). Body/depth/recursion caps
  are enforced upstream by `radar-fetch`.
- **`#![forbid(unsafe_code)]`.**

## Fixture policy (§45)

Every enabled source needs at least one list fixture, one detail fixture (if it
uses detail), and one golden expectation. Site-specific selectors added in M6
must come with a sanitized fixture, not a live-HTML hack.
