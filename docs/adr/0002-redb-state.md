# ADR-0002 — redb for embedded state

- Status: Accepted
- Date: 2026-08-08
- Decider: Deve
- Supersedes: none

## Context

State (§22) needs: canonical fingerprints, first/last seen, source-health
history, change-detection state, schema version. It must be embedded (no daemon),
ACID, and fit a single static binary.

## Decision

Use `redb` as the state store. It is pure Rust, embedded, ACID, and needs no
database daemon — suitable for a single static binary. The state data model does
not require SQL.

## Alternatives considered

- `sled`: rejected — still beta, API churn.
- SQLite (via `rusqlite`): rejected — pulls a C dependency, complicates the musl
  static build (ADR-0003).
- JSON/TOML files: rejected — no atomic multi-key transactions; change detection
  needs consistent snapshots.

## Consequences

`redb` is a pure-Rust dependency, keeping the static musl build clean. Migrations
(§65) must be transactional. State is rebuildable from a scan, but `first_seen`
and media history must not be silently lost.
