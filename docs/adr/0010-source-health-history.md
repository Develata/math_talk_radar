# ADR-0010 — Persist source-health history (multi-record + timestamp)

- Status: Draft (awaiting Deve sign-off)
- Date: 2026-08-16
- Decider: Deve
- Supersedes: none (reconciles §22/§23 plan text "source-health history" with
  the v0.1 code, which never persists any health record)

## Context

§22 (`docs/plan/07_state_change_detection.md`, line 9) specifies the state
store must hold "source-health history." §65 adds: "`first_seen` and media
history must not be silently lost (state is rebuildable but history should not
vanish without notice)."

The v0.1 implementation diverges in two ways:

1. **Never persisted.** `Repository::store_source_health` exists
   (`crates/radar-state/src/repository.rs:319`) but the scan path never calls
   it. `scan_engine.rs` produces `SourceHealth` per source, places it in
   `ScanOutput.source_health`, and emits it to stdout — the `SOURCE_HEALTH`
   redb table is always empty. A restart loses every health observation.

2. **No history, only latest.** Even if `store_source_health` were called, it
   uses `table.insert(source_id, bytes)`, which overwrites the previous entry.
   `SourceHealth` has no timestamp field, so there is no way to distinguish
   observations over time. The table can hold at most one record per source —
   "latest state," not "history."

This is R9-M06 from the round-9 pre-release audit. Deve confirmed (2026-08-16)
the fix scope is (b): real history with a timestamp field and multi-record
storage, requiring a state schema bump and migration.

### Why this is a §12 sign-off item

Changing the persisted shape of `SOURCE_HEALTH` (key scheme + value schema) is
a persisted-data semantics change. Per `AGENTS.md` §12, that requires explicit
Deve sign-off before implementation. This ADR is the sign-off artifact.

## Decision

### 1. Add `recorded_at` to `SourceHealth`

```rust
pub struct SourceHealth {
    pub source: String,
    pub status: SourceStatus,
    pub duration_ms: u64,
    pub requests: u32,
    pub events: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<DateTime<Utc>>,   // NEW
}
```

`recorded_at` is the scan timestamp at which the observation was taken. The
scan path always sets it to `Some(now)` (the same `now` used for `store_scan`).
It is `Option` + `#[serde(default, skip_serializing_if = "Option::is_none")]`
so the generated JSON schema (via `schemars`) lists it as **optional, not
required** — keeping `schema_version = "1.0"` compatible per §64. Existing
consumers that ignore unknown fields are unaffected; the field is simply
absent if an older code path produces a `SourceHealth` without it.

The persisted form is always `Some` (the scan path stamps it); the `Option`
exists only for schema-compatibility of the public JSON shape, not because
the value is ever genuinely absent in practice.

### 2. Change `SOURCE_HEALTH` key scheme to composite `"{source}\x00{recorded_at}"`

```rust
// Before (v2): key = source_id, insert overwrites → latest only.
// After  (v3): key = "{source_id}\x00{recorded_at_rfc3339}", insert appends → history.
pub const SOURCE_HEALTH: TableDefinition<&str, &[u8]> = TableDefinition::new("source_health");
```

The redb table type stays `&str → &[u8]` (no redb type change). The NUL
separator is safe because source ids are TOML string values that cannot
contain NUL. `recorded_at` is formatted as RFC 3339 UTC (e.g.
`2026-08-16T13:45:00Z`); lexicographic sort of the composite key yields
chronological order per source (RFC 3339 with fixed-width fields sorts
correctly as a string).

### 3. Bump `STATE_SCHEMA_VERSION` 2 → 3

The persisted shape of `SOURCE_HEALTH` changes semantically (key format +
value schema). `STATE_SCHEMA_VERSION` is the lever for that (independent of
the public JSON `schema_version`, per ADR-0002).

### 4. Migration v2 → v3

The migration is **additive and lossless**:

- `SOURCE_HEALTH` was never written by the scan path, so on any real v2
  database the table is empty. The migration opens the table (no-op if
  present) and bumps the version row.
- For defensive correctness: if a v2 database has legacy records (keyed by
  bare source id, serialized `SourceHealth` without `recorded_at`), the
  migration reads them, stamps `recorded_at = migration_time` (best
  available), re-inserts under the composite key, and removes the old key.
  Deserialization of the old shape (missing `recorded_at`) is handled by a
  migration-specific serde shim that defaults the field, not by making
  `recorded_at` optional in the live struct.
- Transactional: a failure rolls back, leaving the v2 database intact.

**Default chosen:** stamp `recorded_at = migration_time` and re-insert. The
scan path never wrote this table, so legacy records are a purely defensive
case; stamping preserves them losslessly rather than dropping. Override at
sign-off if dropping is preferred (near-zero risk either way).

### 5. Wire the scan path via a single transactional `store_scan_bundle`

The scan path must persist events AND source health AND purge expired
tombstones/health-history in ONE redb transaction. Separate
`store_source_health` calls cannot share a transaction with `store_scan`, so
introduce a bundle API:

```rust
pub fn store_scan_bundle(
    &self,
    events: &[Event],
    source_health: &[SourceHealth],
    now: DateTime<Utc>,
) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError>
```

This method atomically: (a) compares/stores event observations (current
`store_scan` logic), (b) appends each `source_health` record under its
composite key, (c) purges expired tombstones (ST-16) and expired health
records (>90 days). The `--no-state` path skips persistence (consistent with
current `store_scan`). A bundle failure is a state-fatal error (exit 5),
consistent with the current `store_scan` failure policy (CLI-21).

This bundle API also creates the correct integration point for B06 (source-
scoped observation/cancellation semantics), so the state-v3 design can absorb
B06 without a further API change.

### 6. Bounded retention (prevent unbounded growth)

Without retention, `SOURCE_HEALTH` grows by one record per source per scan,
forever. Adopt a **time-windowed retention** of 90 days, matching
`TOMBSTONE_RETENTION_DAYS` (`repository.rs:28`, ST-16). Records older than 90
days are purged during `store_scan_bundle` (same transaction that writes new
records). This bounds the table to ≈ `num_sources × (90 / scan_interval_days)` records
and keeps a single retention constant governing both tombstones and health
history.

A `--doctor` or future `health history <source>` command reads the history;
the scan path only writes it.

**Default chosen:** 90 days (consistency with ST-16). Override at sign-off if a
different window is preferred.

### 7. Public JSON schema: add `recorded_at` as optional (v0.x-compatible)

Per §64, v0.x may add optional fields without bumping the public
`schema_version = "1.0"`. The `scan` output's `source_health` array entries
will include `recorded_at` (the scan timestamp). Existing consumers that
ignore unknown fields are unaffected; consumers that want the timestamp can
opt in. No public schema bump required for v0.1.0.

**Default chosen:** optional field, `schema_version` stays `"1.0"`. This
follows §64's stated v0.x path and AGENTS.md global priority #4 (public
contract stability — avoid a `"1.1"` bump unless a field is renamed or
removed). Override at sign-off if a required-field bump is preferred.

## Alternatives considered

- **(a) Wire up `store_source_health` as-is (latest-state only).** Rejected by
  Deve: the plan says "history," and latest-state loses every observation on
  the next scan. Insufficient for diagnosing intermittent source failures over
  time.
- **Separate `SOURCE_HEALTH_HISTORY` table, keep `SOURCE_HEALTH` for latest.**
  Rejected: two tables with overlapping content doubles the write path and
  the retention logic. The composite-key scheme gives both "latest" (last
  record per source) and "history" (all records) from one table.
- **Event-sourced log (append-only, no retention).** Rejected: unbounded
  growth is unacceptable for an embedded local DB; 90-day windowed retention
  matches the existing tombstone policy and keeps the table bounded.

## Consequences

- `STATE_SCHEMA_VERSION` → 3. A v2 database migrates forward automatically
  (transactional, lossless). A v3 database cannot be opened by a v2 binary
  (forward-only, per `run_migrations` backward refusal).
- `SOURCE_HEALTH` table grows to a bounded steady state (≈ 90 days × scan
  frequency × source count).
- The public `scan` JSON gains `recorded_at` (optional, `Option` + `skip_serializing_if`)
  in each `source_health` entry. `schema_version` stays `"1.0"` (optional field
  addition, §64).
- A future `doctor` / `health history` command can read the history without a
  further schema change.
- Implementing this ADR requires: (1) `SourceHealth` struct change
  (`Option<DateTime<Utc>>` + serde attrs), (2) key scheme + `store_scan_bundle`
  transactional API (replaces separate `store_scan` + `store_source_health`) +
  a `list_source_health(source)` read API, (3) v2→v3 migration, (4) scan-path
  wiring to `store_scan_bundle`, (5) retention purge inside `store_scan_bundle`,
  (6) tests (migration, retention, read-back, `--no-state` skip).
- **State-v3 coordination (GPT-Pro R9 re-review):** B06, H08, H09, and M06 all
  affect persisted observations or change detection. This ADR should NOT be
  implemented in isolation. The four should be designed together before
  assigning `STATE_SCHEMA_VERSION = 3`, to avoid an immediate v4 bump if H08
  or B06 changes the storage model after M06 has already claimed v3.

## Sign-off request

This ADR changes persisted-data semantics (§12): `SOURCE_HEALTH` key scheme,
value schema, and `STATE_SCHEMA_VERSION` 2 → 3. The three sub-decisions above
each carry a **default grounded in precedent** (ST-16 retention window, §64
optional-field policy, lossless migration). Deve: review the defaults; if any
is acceptable as-is, sign off and implementation proceeds. If a different
value is preferred on any sub-decision, note it and the ADR + implementation
will adjust. Implementation is blocked until sign-off per AGENTS.md §12.
