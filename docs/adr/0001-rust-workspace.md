# ADR-0001 — Rust workspace with strict crate boundaries

- Status: Accepted
- Date: 2026-08-08
- Decider: Deve
- Supersedes: none

## Context

The system mixes pure domain logic, HTTP fetching, document parsing,
persistence, and CLI composition (§10, §11). Putting these in one crate would
let network code leak into parsers and ranking into the fetch layer, exactly the
anti-patterns §74 forbids.

## Decision

Use a small Cargo workspace with four library crates and one binary:

- `radar-core` — pure domain, no I/O.
- `radar-fetch` → core — HTTP only.
- `radar-adapters` → core — pure parsing, no network.
- `radar-state` → core — persistence only.
- `math_talk_radar` (cli) → all four — the only composition root.

Dependency edges enforce the boundaries at compile time: core forbids
`reqwest`/`redb`/`scraper`; adapters and state forbid `reqwest`; fetch forbids
`scraper`.

## Alternatives considered

- Single crate: rejected — no compile-time boundary enforcement; §74 risk.
- Monorepo with many tiny crates: rejected — premature; four crates match the
  four concerns.

## Consequences

A change to the shared `FetchedDocument`/`SourceAdapter` contract (§13) touches
core, which both fetch and adapters depend on — by design, since that contract
is the boundary. File cohesion (§40) is monitored per crate.
