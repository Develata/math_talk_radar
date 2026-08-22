//! redb table definitions (§22, ADR-0002, ADR-0011). Keys are `&str` (event id,
//! source id, composite keys); values are `&[u8]` serde_json-serialized domain
//! types. The schema version table stores the persisted `STATE_SCHEMA_VERSION`.

use chrono::{DateTime, SecondsFormat, Utc};
use redb::TableDefinition;

use crate::changes::ChangeRecord;

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
///
/// v4: canonicalized every persisted timestamp key to UTC RFC3339 with exactly
/// nine fractional digits, and replaced the raw `CHANGE_LOG` detail suffix with
/// a fixed-size BLAKE3 digest. This makes lexicographic order equal chronological
/// order within the same second and keeps externally sourced details from
/// creating unbounded database keys. The v3→v4 migration is transactional and
/// lossless.
pub const STATE_SCHEMA_VERSION: u32 = 4;

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
/// `"{source_id}\x00{recorded_at_fixed_rfc3339}"` so each scan appends a new
/// record rather than overwriting. [`timestamp_key`] always emits UTC RFC3339
/// with exactly nine fractional digits, so lexicographic order is chronological
/// even when observations differ only within one second. Value is a
/// serde_json-serialized [`radar_core::SourceHealth`] (with `recorded_at`
/// stamped by the scan path). Purged after
/// [`super::repository::RETENTION_DAYS`] (90 days).
pub const SOURCE_HEALTH: TableDefinition<&str, &[u8]> = TableDefinition::new("source_health");

/// Change log (ADR-0011 §3, R9-H08). Keyed by composite
/// `"{detected_at_fixed_rfc3339}\x00{event_id}\x00{kind}\x00{detail_digest}"`
/// so records sort chronologically across all events, and multiple same-kind
/// changes in one scan remain distinct. The timestamp-first layout enables
/// `range(..cutoff)` expiry in O(log n + expired); the fixed-size digest avoids
/// placing unbounded external text directly in a database key. Value is a
/// serde_json-serialized [`super::changes::ChangeRecord`]. Purged after
/// [`super::repository::RETENTION_DAYS`] (90 days).
pub const CHANGE_LOG: TableDefinition<&str, &[u8]> = TableDefinition::new("change_log");

/// Schema version. Single row keyed by `"version"`.
pub const SCHEMA_VERSION: TableDefinition<&str, u32> = TableDefinition::new("schema_version");

/// Canonical timestamp representation for persisted ordered keys. Chrono's
/// default `to_rfc3339()` uses variable fractional precision; that makes
/// lexicographic order disagree with time order inside a second.
pub(crate) fn timestamp_key(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

/// Composite key for one source-health observation.
pub(crate) fn source_health_key(source: &str, recorded_at: DateTime<Utc>) -> String {
    format!("{source}\u{0}{}", timestamp_key(recorded_at))
}

/// Composite key for one change record. The detail digest is deterministic and
/// fixed-width; two different same-kind changes therefore do not overwrite one
/// another, while a duplicate record remains idempotent.
pub(crate) fn change_log_key(record: &ChangeRecord) -> String {
    let detail = record.detail.as_deref().unwrap_or("");
    let detail_digest = radar_core::deterministic_id(&[detail]);
    format!(
        "{}\u{0}{}\u{0}{}\u{0}{}",
        timestamp_key(record.detected_at),
        record.event_id.0,
        record.kind.as_str(),
        detail_digest
    )
}
