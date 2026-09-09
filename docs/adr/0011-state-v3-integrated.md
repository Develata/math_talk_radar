# ADR-0011 — Integrated state-v3 (source-health history, change log, tombstones, first_seen)

- Status: Accepted (Deve sign-off 2026-08-17 — defaults approved: RETENTION_DAYS=90,
  §64 optional-field path, lossless additive migration)
- Date: 2026-08-17
- Decider: Deve
- Supersedes: ADR-0010 (rolled into this ADR; ADR-0010's M06 content is
  preserved here as Decision §1–§3)
- Related findings: R9-B06, R9-H08, R9-H09, R9-M06 (Round 9 pre-release audit)

## Context

Four Round-9 findings all touch persisted observations or change-detection
state. Implementing them one at a time would force repeated
`STATE_SCHEMA_VERSION` bumps and overlapping migrations. Per the GPT-Pro R9
re-review coordination note (recorded at the bottom of ADR-0010), they must
be designed together before claiming v3. This ADR is that integrated design.

### The four findings

**R9-M06 — source-health history never persisted.** `Repository::
store_source_health` exists (`crates/radar-state/src/repository.rs:319`) but
the scan path never calls it. `scan_engine.rs` produces `SourceHealth` per
source, places it in `ScanOutput.source_health`, and emits it to stdout — the
`SOURCE_HEALTH` redb table is always empty. A restart loses every health
observation. Even if `store_source_health` were called, it uses
`table.insert(source_id, bytes)`, which overwrites the previous entry.
`SourceHealth` has no timestamp field. The table holds at most one record per
source — "latest state," not the "history" §22 (`docs/plan/
07_state_change_detection.md:9`) requires.

**R9-B06 — source-scoped observation semantics.** A health observation is
meaningful only with its source scope and timestamp. The current `SourceHealth`
struct has no `recorded_at`; the table key has no source+time discrimination.
Diagnosing intermittent source failures (e.g. a source that fails every other
scan) is impossible without per-scan history. B06 and M06 are the same defect
viewed from different angles; fixing one without the other leaves the table
useless.

**R9-H08 — change log and media history not persisted.** §65 (`docs/plan/
07_state_change_detection.md:25`) states: "`first_seen` and media history must
not be silently lost (state is rebuildable but history should not vanish
without notice)." `detect_changes` (`crates/radar-state/src/changes.rs`)
emits `ChangeRecord`s (EventAdded, MediaAdded, MediaRemoved,
EventCancelled, …) and `store_scan` returns them to the scan path, which
prints them to stdout — but they are never written to the DB. A restart loses
every change observation. `first_seen_at` survives because it is a field on
the persisted `Event`; the *history of changes* does not. This is the
"media history must not be silently lost" half of §65.

**R9-H09 — tombstone retention semantics undocumented.** The
`CANCELLED_EVENTS` tombstone (ST-16, `repository.rs:30`) correctly preserves
`first_seen_at` when a cancelled event reappears, and the 90-day retention
(`TOMBSTONE_RETENTION_DAYS = 90`, `repository.rs:28`) bounds the table. But
the semantic invariants — "tombstone guarantees `first_seen_at` restoration
within the retention window; reappearance past the window is a genuinely new
event; purge happens in the same transaction as the scan write" — exist only
as inline code comments, not as a contract. A future refactor could silently
weaken them. H09 asks for the invariants to be recorded as a contract so a
regression is a contract violation, not just a code change.

### Why this is a §12 sign-off item

This ADR changes persisted-data semantics: a new `CHANGE_LOG` table, a
key-scheme + value-schema change to `SOURCE_HEALTH`, and a
`STATE_SCHEMA_VERSION` bump 2 → 3. Per `AGENTS.md` §12, persisted-data
semantics changes require explicit Deve sign-off. This ADR is the sign-off
artifact.

## Design principles

1. **One schema bump for all four.** v2 → v3 in a single transactional
   migration. No v4 follow-up for these four.
2. **Additive, lossless migration.** No existing row is dropped or
   reinterpreted destructively. The new `CHANGE_LOG` table starts empty.
   `SOURCE_HEALTH` legacy rows (defensive case — the scan path never wrote
   them) are re-keyed and stamped.
3. **One transactional write primitive.** The scan path persists events,
   change records, source-health observations, and purges expired tombstones
   + expired health records in ONE redb transaction. Split writes cannot
   share a transaction and would leave the DB half-updated on a mid-scan
   failure.
4. **Bounded retention everywhere.** Every append-only table has a retention
   window so the embedded DB cannot grow without bound. Tombstones 90 days
   (existing); health history 90 days (new, same constant); change log 90
   days (new, same constant). A single `RETENTION_DAYS` governs all three.
5. **Public JSON stays `"1.0"`.** New persisted fields surface in the public
   `scan` JSON only as OPTIONAL additions (§64 v0.x path). No public schema
   bump. The state schema version is independent of the public JSON schema
   version (ADR-0002).
6. **Invariants are recorded, not just implemented.** H09's tombstone
   semantics and the first_seen-preservation rule are written as numbered
   invariants below so a regression is a contract violation.

## Decision

### §1. Add `recorded_at` to `SourceHealth` (M06, B06)

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
scan path always sets it to `Some(now)` (the same `now` used for
`store_scan_bundle`). It is `Option` + `#[serde(default,
skip_serializing_if = "Option::is_none")]` so the public JSON schema (via
`schemars`) lists it as **optional, not required** — keeping
`schema_version = "1.0"` compatible per §64. The persisted form is always
`Some`; the `Option` exists only for public-schema compatibility, not because
the value is ever genuinely absent in practice.

### §2. Change `SOURCE_HEALTH` key scheme to composite `"{source}\x00{recorded_at}"` (M06, B06)

```rust
// Before (v2): key = source_id, insert overwrites → latest only.
// After  (v3): key = "{source_id}\x00{recorded_at_rfc3339}", insert appends → history.
pub const SOURCE_HEALTH: TableDefinition<&str, &[u8]> = TableDefinition::new("source_health");
```

The redb table type stays `&str → &[u8]` (no redb type change). The NUL
separator is safe because source ids are TOML string values that cannot
contain NUL. `recorded_at` is formatted as RFC 3339 UTC (e.g.
`2026-08-17T13:45:00Z`); lexicographic sort of the composite key yields
chronological order per source (RFC 3339 with fixed-width fields sorts
correctly as a string). The "latest" record per source is the last one under
the `{source}\x00` prefix.

### §3. Add `CHANGE_LOG` table for persisted change records (H08)

```rust
/// Change records (§23) keyed by `"{detected_at}\x00{event_id}\x00{kind}"`.
/// Value is a serde_json-serialized [`ChangeRecord`]. The composite key
/// gives chronological order across all events, and per-event order within
/// a scan. Bounded by [`RETENTION_DAYS`] (purged in `store_scan_bundle`).
pub const CHANGE_LOG: TableDefinition<&str, &[u8]> = TableDefinition::new("change_log");
```

`ChangeRecord` already exists (`crates/radar-state/src/changes.rs:27`) and is
already `Serialize`/`Deserialize`. No struct change is needed — the records
that `detect_changes` already produces are simply persisted in addition to
being returned to the scan path. The key scheme `"{detected_at}\x00{event_id}
\x00{kind}"` sorts chronologically; `kind` is appended so multiple changes
on the same event in the same scan (e.g. `event_updated` + `media_added`)
do not collide.

NUL separators are safe: `detected_at` is RFC 3339 (no NUL), `event_id` is a
URL-derived canonical string (no NUL after §00 normalization), and `kind` is
a fixed snake_case enum variant (no NUL). The `detail` field (a URL for
media changes, a talk id for schedule changes) stays inside the
serialized value, not the key, so its content cannot break the key ordering.

A future `changes` / `changes --since <ts>` command reads this table. The
scan path only writes it.

### §4. Bump `STATE_SCHEMA_VERSION` 2 → 3

The persisted shape of `SOURCE_HEALTH` changes semantically (key format +
value schema) and `CHANGE_LOG` is new. `STATE_SCHEMA_VERSION` is the lever
for both (independent of the public JSON `schema_version`, per ADR-0002).

### §5. Migration v2 → v3 (additive, lossless, single transaction)

The migration runs inside one write transaction; a failure rolls back,
leaving the v2 database intact.

- `CHANGE_LOG`: created by `open_table` (no-op if present; empty on a v2 DB).
- `SOURCE_HEALTH`: on any real v2 database the table is empty (the scan path
  never wrote it). The migration opens the table (no-op if present) and bumps
  the version row. For defensive correctness: if a v2 database has legacy
  records (keyed by bare source id, serialized `SourceHealth` without
  `recorded_at`), the migration reads them, stamps `recorded_at =
  migration_time` (best available), re-inserts under the composite key, and
  removes the old key. Deserialization of the old shape (missing
  `recorded_at`) is handled by `#[serde(default)]` on the field — no
  migration-specific serde shim needed.
- `EVENTS`, `CANCELLED_EVENTS`: unchanged shape; `open_table` no-op.
- Version row bumped 2 → 3.

**Default chosen:** stamp `recorded_at = migration_time` and re-insert legacy
`SOURCE_HEALTH` rows. The scan path never wrote this table, so legacy rows
are a purely defensive case; stamping preserves them losslessly rather than
dropping. Override at sign-off if dropping is preferred (near-zero risk
either way).

### §6. Wire the scan path via a single transactional `store_scan_bundle`

The scan path must persist events AND change records AND source health AND
purge expired tombstones/health/change-log in ONE redb transaction. Separate
calls cannot share a transaction, so introduce a bundle API:

```rust
pub fn store_scan_bundle(
    &self,
    events: &[Event],
    source_health: &[SourceHealth],
    now: DateTime<Utc>,
) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError>
```

This method atomically, in one `begin_write`/`commit`:
1. Reads all previous events (current `store_scan` logic).
2. Runs `detect_changes` → produces `Vec<ChangeRecord>` (existing logic).
3. Upserts each current event, preserving `first_seen_at` and stamping
   `last_seen_at = now` (existing logic, including ST-16 tombstone
   restoration).
4. Prunes events absent from the current scan: writes a tombstone preserving
   `first_seen_at`, removes the event row (existing ST-16 logic).
5. **NEW:** appends each `ChangeRecord` to `CHANGE_LOG` under its composite
   key.
6. **NEW:** appends each `source_health` record to `SOURCE_HEALTH` under its
   composite key. The scan path must stamp each `SourceHealth.recorded_at =
   Some(now)` before calling (enforced by a debug_assert + a defensive
   fallback that stamps missing entries).
7. Purges expired rows from `CANCELLED_EVENTS`, `SOURCE_HEALTH`, and
   `CHANGE_LOG` (all older than `now - RETENTION_DAYS`), in the same
   transaction.

The `--no-state` path skips persistence entirely (consistent with current
`store_scan`). A bundle failure is a state-fatal error (exit 5), consistent
with the current `store_scan` failure policy (CLI-21).

`store_scan` and `store_source_health` are deprecated in favor of
`store_scan_bundle`. They remain for `--no-state` read-only opens and for
tests that exercise the single-event path, but the scan path uses only
`store_scan_bundle`.

### §7. Bounded retention — single constant (M06, H08, H09)

Without retention, `SOURCE_HEALTH` and `CHANGE_LOG` grow without bound. Adopt
a single retention window for all three append-only/retention tables:

```rust
/// Retention window (days) for cancelled-event tombstones (ST-16),
/// source-health history (R9-M06), and the change log (R9-H08). A single
/// constant governs all three so they age out together and the embedded DB
/// stays bounded at ≈ num_sources × (90 / scan_interval_days) health
/// records, plus ≈ changes_per_scan × (90 / scan_interval_days) change
/// records, plus the live event set.
const RETENTION_DAYS: i64 = 90;
```

The existing `TOMBSTONE_RETENTION_DAYS` is renamed to `RETENTION_DAYS` and
applied to all three tables. 90 days covers the typical academic-year cycle
(a talk announced for a term, removed when the term ends, re-listed the
following year) and matches the existing tombstone precedent. Purge happens
inside `store_scan_bundle` (same transaction as the write), so a crash mid-
purge leaves the pre-purge state intact.

**Default chosen:** 90 days (consistency with ST-16). Override at sign-off if
a different window is preferred.

### §8. Tombstone and first_seen invariants (H09) — recorded as a contract

The following invariants are the contract for `CANCELLED_EVENTS` and the
`first_seen_at` field. A regression in any of these is a contract violation,
not just a code change, and must be caught by a test.

- **INV-1 (first_seen preservation).** For an event present in consecutive
  scans, `first_seen_at` is never overwritten by the current scan's `now`.
  Only a genuinely new event (no previous row, no unexpired tombstone)
  receives `first_seen_at = now`.
- **INV-2 (tombstone restoration).** If an event was pruned (cancelled) and
  reappears within `RETENTION_DAYS` of `cancelled_at`, its `first_seen_at`
  is restored from the tombstone, not reset to the reappearance scan time.
  The tombstone is removed in the same transaction that restores the event.
- **INV-3 (tombstone expiry).** A reappearance past `RETENTION_DAYS` of
  `cancelled_at` is treated as a genuinely new event (`first_seen_at = now`).
  The expired tombstone is purged in the same transaction.
- **INV-4 (tombstone purge atomicity).** Tombstone purge (expired and
  restored) happens inside the `store_scan_bundle` transaction. A crash
  before commit leaves all tombstones intact; after commit, the purged set
  is gone and the retained set is consistent with the new scan.
- **INV-5 (tombstone content).** A tombstone stores exactly `first_seen_at`
  and `cancelled_at`. No other field is persisted in the tombstone; in
  particular, the full event is NOT recoverable from the tombstone — only
  the `first_seen_at` is preserved. This is intentional: the event is
  cancelled, and a reappearance is treated as the same event only for
  `first_seen_at` purposes, not for restoring its old title/speakers/media.

### §9. Public JSON schema: optional fields, `"1.0"` stays (§64)

Per §64, v0.x may add optional fields without bumping the public
`schema_version = "1.0"`.

- `scan` output's `source_health` array entries gain `recorded_at` (optional,
  `Option` + `skip_serializing_if`). Existing consumers that ignore unknown
  fields are unaffected.
- A future `changes` / `changes --since <ts>` command would emit
  `ChangeRecord`s as-is (they are already `JsonSchema`-derived). No `scan`
  output change for v0.1.
- No public schema bump required for v0.1.0.

**Default chosen:** optional fields, `schema_version` stays `"1.0"`. This
follows §64's stated v0.x path and AGENTS.md global priority #4 (public
contract stability — avoid a `"1.1"` bump unless a field is renamed or
removed). Override at sign-off if a required-field bump is preferred.

## Cross-cutting invariants

- **TXN-1 (bundle atomicity).** Events, change log, source health, and all
  purges are written in ONE `store_scan_bundle` transaction. The scan path
  MUST NOT call `store_event` or `store_source_health` separately for the
  same scan.
- **TXN-2 (no partial write on failure).** A `store_scan_bundle` failure
  rolls back the entire transaction. The scan path surfaces this as exit 5
  (CLI-21). No half-written state is observable.
- **DET-1 (deterministic `now`).** `now` is caller-supplied; the repository
  never reads a wall clock. Identical inputs produce identical persisted
  state (existing invariant, preserved).
- **DET-2 (deterministic change order).** `detect_changes` output is sorted
  by `(event_id, kind)` (`changes.rs:166`). `CHANGE_LOG` keys sort
  chronologically by `detected_at`, then by `event_id`, then by `kind` —
  stable across runs for the same scan.
- **RO-1 (read-only refusal).** `store_scan_bundle` on a read-only repo
  returns `StateError::ReadOnly`. The `--no-state` path (STATE-004) opens
  read-only and never calls the bundle.
- **MIG-1 (forward-only).** A v3 database cannot be opened by a v2 binary
  (`run_migrations` backward refusal, `migrations.rs:37`). A v2 database
  migrates forward automatically and losslessly.

## Alternatives considered

- **Implement M06 alone (ADR-0010 as-is), then B06/H08/H09 in a v4.**
  Rejected: the GPT-Pro R9 re-review coordination note explicitly warns this
  forces an immediate v4 bump. One bump for all four is cheaper and safer.
- **Separate `SOURCE_HEALTH_HISTORY` table, keep `SOURCE_HEALTH` for latest.**
  Rejected: two tables with overlapping content doubles the write path and
  the retention logic. The composite-key scheme gives both "latest" (last
  record per source) and "history" (all records) from one table.
- **Event-sourced log (append-only, no retention).** Rejected: unbounded
  growth is unacceptable for an embedded local DB; 90-day windowed retention
  matches the existing tombstone policy and keeps every append-only table
  bounded.
- **Persist full `Event` in the tombstone (not just `first_seen_at`).**
  Rejected (INV-5): a reappearance is the same event only for `first_seen_at`
  purposes. Restoring the old title/speakers/media would mask a genuine
  content change. The tombstone is a `first_seen_at` preserver, not an event
  time machine.
- **Separate `CHANGE_LOG` retention from tombstone retention.** Rejected:
  two windows to tune, two purge paths, two constants to keep consistent. A
  single `RETENTION_DAYS` is simpler and matches the existing precedent.

## Consequences

- `STATE_SCHEMA_VERSION` → 3. A v2 database migrates forward automatically
  (transactional, lossless). A v3 database cannot be opened by a v2 binary.
- `SOURCE_HEALTH` table grows to a bounded steady state (≈ 90 days × scan
  frequency × source count).
- `CHANGE_LOG` table grows to a bounded steady state (≈ 90 days × scan
  frequency × changes-per-scan).
- `CANCELLED_EVENTS` retention constant renamed `TOMBSTONE_RETENTION_DAYS`
  → `RETENTION_DAYS` (same value, 90).
- The public `scan` JSON gains `recorded_at` (optional) in each
  `source_health` entry. `schema_version` stays `"1.0"` (§64).
- A future `doctor` / `health history <source>` / `changes --since <ts>`
  command can read the new tables without a further schema change.
- Implementing this ADR requires: (1) `SourceHealth` struct change
  (`Option<DateTime<Utc>>` + serde attrs); (2) `CHANGE_LOG` table + key
  scheme; (3) `SOURCE_HEALTH` composite key; (4) `store_scan_bundle`
  transactional API (replaces separate `store_scan` +
  `store_source_health` on the scan path) + `list_source_health(source)` +
  `list_changes(since)` read APIs; (5) v2→v3 migration; (6) scan-path wiring
  to `store_scan_bundle` with `recorded_at` stamping; (7) retention purge
  inside `store_scan_bundle` for all three tables; (8) tests covering each
  invariant INV-1..INV-5, TXN-1/TXN-2, the migration, and the `--no-state`
  skip.
- ADR-0010 is superseded by this ADR. Its M06 content is preserved here as
  §1, §2, and the migration/retention defaults.

## Sign-off request

This ADR changes persisted-data semantics (§12): a new `CHANGE_LOG` table, a
`SOURCE_HEALTH` key-scheme + value-schema change, and `STATE_SCHEMA_VERSION`
2 → 3. The sub-decisions above each carry a **default grounded in precedent**
(ST-16 retention window, §64 optional-field policy, lossless migration,
single retention constant). Deve: review the defaults; if any is acceptable
as-is, sign off and implementation proceeds. If a different value is
preferred on any sub-decision, note it and the ADR + implementation will
adjust. Implementation is blocked until sign-off per AGENTS.md §12.
