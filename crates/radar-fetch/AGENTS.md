# AGENTS.md — `crates/radar-fetch`

## Purpose

The HTTP layer: client, concurrency, timeouts, retry, robots policy, request
budget. Produces `radar_core::FetchedDocument` for the coordinator and adapters.

## Authority

Follows `docs/plan/05_fetching_reliability.md` and `docs/plan/11_security.md`.

## Hard boundaries

- **HTTP only.** No event ranking, no parsing business logic, no dedup.
- **No `scraper`, no `redb`, no `feed-rs`.** Parsing belongs to `radar-adapters`.
- **rustls only.** `reqwest` is configured with `default-features = false` +
  `rustls-tls`. Never depend on system OpenSSL (§37, ADR-0003).
- **No cookies, no auth, no saved bodies beyond the bounded `FetchedDocument`.**
- **`#![forbid(unsafe_code)]`.**

## Contract highlights (§15, §16)

- Defaults: global concurrency 8, per-host 2, connect 5s, request 15s, global
  deadline 30s, redirect 5, max retry 1, max body 4 MiB.
- Retry only: connection reset, transient network, 408, 429, 5xx. Never retry
  400/401/403/404/410, robots-denied, parse failure.
- 429 honors `Retry-After` but never breaches the global deadline.
- UA: `math_talk_radar/<version> (+public-repository)`. `respect_robots = true`
  always; no robots bypass is ever provided.
