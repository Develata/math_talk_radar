//! redb table definitions (§22, ADR-0002, ADR-0011). Keys are `&str` (event id,
//! source id, composite keys); values are `&[u8]` serde_json-serialized domain
//! types. The schema version table stores the persisted `STATE_SCHEMA_VERSION`.

use redb::TableDefinition;

/// Current state DB schema version. Independent of the public JSON
/// `schema_version`. Bumped on any destructive or semantic change to the
/// persisted shape; migrations live in [`super::migrations`].
///
/// v2 (ST-16): added `CANCELLED_EVENTS` tombstone table to preserve
/// `first_seen_at` when a cancelled event reappears in a later scan.
///
/// v3 (ADR-0011): split `SOURCE_HEALTH` key to composite
/// `"{source}\x00{recorded_at}"` for per-scan history (R9-M06/B06); added
/// `CHANGE_LOG` table persisting `ChangeRecord`s (R9-H08); unified retention
/// to a single `RETENTION_DAYS` constant. Additive, lossless migration.
pub const STATE_SCHEMA_VERSION: u32 = 3;

/// Events keyed by their stable `EventId` string. Value is a serde_json-serialized
/// [`radar_core::Event`] (which carries `first_seen_at` / `last_seen_at`).
pub const EVENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("events");

/// Tombstones for cancelled events (ST-16, ADR-0011 INV-1..INV-5). Keyed by
/// `EventId` string. Value is a serde_json-serialized
/// [`super::repository::CancelledEventTombstone`] holding the `first_seen_at`
/// and `cancelled_at` timestamps. When a cancelled event reappears within
/// [`super::repository::RETENTION_DAYS`], its `first_seen_at` is restored from
/// the tombstone instead of being reset to the current scan time. Tombstones
/// are purged after the retention window (90 days).
pub const CANCELLED_EVENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("cancelled_events");

/// Source health history (ADR-0011 §2). Keyed by composite
/// `"{source_id}\x00{recorded_at_rfc3339}"` so each scan appends a new record
/// rather than overwriting. Lexicographic sort of the composite key yields
/// chronological order per source — this relies on `recorded_at` being a
/// `DateTime<Utc>` (fixed `Z` offset, zero-padded RFC3339); a `FixedOffset`
/// or raw-string timestamp would silently break the ordering. Value is a
/// serde_json-serialized [`radar_core::SourceHealth`] (with `recorded_at`
/// stamped by the scan path). Purged after
/// [`super::repository::RETENTION_DAYS`] (90 days).
pub const SOURCE_HEALTH: TableDefinition<&str, &[u8]> = TableDefinition::new("source_health");

/// Change log (ADR-0011 §3, R9-H08). Keyed by composite
/// `"{detected_at_rfc3339}\x00{event_id}\x00{kind}"` so records sort
/// chronologically across all events, and per-event within a scan. The
/// timestamp-first layout enables `range(..cutoff)` expiry in
/// O(log n + expired) — this relies on `detected_at` being a `DateTime<Utc>`
/// (fixed `Z` offset, zero-padded RFC3339); a `FixedOffset` or raw-string
/// timestamp would silently break the ordering. Value is a
/// serde_json-serialized [`super::changes::ChangeRecord`]. Purged after
/// [`super::repository::RETENTION_DAYS`] (90 days).
pub const CHANGE_LOG: TableDefinition<&str, &[u8]> = TableDefinition::new("change_log");

/// Schema version. Single row keyed by `"version"`.
pub const SCHEMA_VERSION: TableDefinition<&str, u32> = TableDefinition::new("schema_version");
