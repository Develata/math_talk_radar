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

## M2 dependency additions (2026-08-09)

### MSRV bump 1.85 → 1.96

`feed-rs` 2.4.0 requires `quick-xml` 0.41, which fixes two HIGH-severity RustSec
advisories (`RUSTSEC-2026-0194`, `RUSTSEC-2026-0195`). `quick-xml` 0.41 in turn
requires Rust 1.96 as its MSRV. The previous MSRV of 1.85 would have pinned us
to `feed-rs` 2.0 + an older `quick-xml` carrying the unfixed advisories. Bumping
the workspace MSRV to 1.96 closes the advisory path and is the floor for all M2
adapter work. The installed toolchain is 1.97.1.

### New workspace dependencies

- `icalendar = { version = "0.17", features = ["parser"] }` — ICS (iCalendar)
  feed parsing for `.ics` sources. The `parser` feature exposes the low-level
  `read_calendar` parser used by the ICS adapter without pulling in the calendar
  construction API.
- `futures = "0.3"` — async combinators (`stream`, `join`, `select`) for the
  fetch engine's concurrency and timeout orchestration in `radar-fetch`.

### New workspace dev-dependency

- `wiremock = "0.6"` — mock HTTP server for `radar-fetch` and `radar-adapters`
  integration tests. Cargo has no `[workspace.dev-dependencies]` table;
  dev-dependencies inherit from `[workspace.dependencies]` via
  `dep.workspace = true` in a member's `[dev-dependencies]` section. Wiremock
  is therefore declared in `[workspace.dependencies]` with a `# Test-only`
  marker and inherited only into member `[dev-dependencies]` blocks. No
  production crate depends on it; it never enters a release binary.

### Version bumps

- `scraper` 0.22 → 0.27 — tracks current `html5ever` and fixes upstream
  parsing bugs. Used only by `radar-adapters`.
- `feed-rs` 2.0 → 2.4 — security fix (pulls `quick-xml` 0.41 for
  `RUSTSEC-2026-0194`/`0195`). Used only by `radar-adapters`.

### Boundary reaffirmation

The M2 additions do not relax §11. `radar-adapters` gains `scraper`, `feed-rs`,
`serde_json`, and `icalendar` — all pure parsing crates, no network. `radar-fetch`
gains `tokio` and `futures` — runtime and combinators, no parsing. Neither crate
gains `reqwest` (stays in `radar-fetch` only) or `scraper` (stays in
`radar-adapters` only). `#![forbid(unsafe_code)]` is unchanged in every crate.
