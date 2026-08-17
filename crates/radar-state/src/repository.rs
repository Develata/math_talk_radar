//! State repository (§22, ADR-0002). `redb`-backed persistence of event
//! fingerprints, first/last seen, and source health. The repository is the
//! only mutation surface for persisted state; change detection (§23) reads
//! previous state through [`Repository::list_events`] and writes the new
//! scan through [`Repository::store_event`].
//!
//! Determinism: `now` is a caller-supplied timestamp — the repository never
//! reads a wall clock, so identical inputs produce identical persisted state.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use radar_core::{Event, EventId, SourceHealth};
use redb::ReadableTable;
use serde::{Deserialize, Serialize};

use crate::changes::{ChangeRecord, detect_changes};
use crate::migrations::run_migrations;
use crate::schema::{
    CANCELLED_EVENTS, CHANGE_LOG, EVENTS, SCHEMA_VERSION, SOURCE_HEALTH, STATE_SCHEMA_VERSION,
};

/// Retention window (days) for cancelled-event tombstones (ST-16),
/// source-health history (ADR-0011 §7), and the change log (ADR-0011 §3).
/// A single constant governs all three so they age out together and the
/// embedded DB stays bounded. 90 days covers the typical academic-year
/// cycle.
const RETENTION_DAYS: i64 = 90;

/// Tombstone for a cancelled event (ST-16, ADR-0011 INV-1..INV-5). Stores the
/// `first_seen_at` of the event at the moment it was pruned, plus the
/// `cancelled_at` timestamp used to age out the tombstone after
/// [`RETENTION_DAYS`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledEventTombstone {
    pub first_seen_at: DateTime<Utc>,
    pub cancelled_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("state io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("state schema mismatch: expected {expected}, found {found}")]
    Schema { expected: u32, found: u32 },
    #[error("state backend error: {0}")]
    Backend(Box<redb::Error>),
    #[error("state read-only: write attempted on a read-only repository")]
    ReadOnly,
    #[error("state serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

// Manual From impls so `?` converts each redb-specific error into the single
// boxed backend variant. Boxed to keep the Err variant small (some redb errors
// embed a ReadTransaction, which is large — clippy::result_large_err).
impl From<redb::DatabaseError> for StateError {
    fn from(e: redb::DatabaseError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::TransactionError> for StateError {
    fn from(e: redb::TransactionError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::TableError> for StateError {
    fn from(e: redb::TableError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::CommitError> for StateError {
    fn from(e: redb::CommitError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::StorageError> for StateError {
    fn from(e: redb::StorageError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}

/// Persistent state repository backed by an embedded `redb` database.
pub struct Repository {
    db: redb::Database,
    path: PathBuf,
    read_only: bool,
}

impl std::fmt::Debug for Repository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repository")
            .field("path", &self.path)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl Repository {
    /// Open or create the state database at `path`, running migrations to the
    /// current schema version. Fails if an existing database is at an
    /// unsupported schema version.
    pub fn open(path: &Path) -> Result<Self, StateError> {
        let db = redb::Database::create(path)?;
        run_migrations(&db).map_err(|e| match e {
            crate::migrations::MigrateError::UnsupportedVersion { found, expected } => {
                StateError::Schema { expected, found }
            }
            crate::migrations::MigrateError::Backend(e) => StateError::Backend(e),
        })?;
        Ok(Self {
            db,
            path: path.to_path_buf(),
            read_only: false,
        })
    }

    /// Open the database in read-only mode. Writes (store_event /
    /// store_scan_bundle) return [`StateError::ReadOnly`]. The database file
    /// is never created or modified. Used by the `--no-state` path (STATE-004).
    pub fn open_read_only(path: &Path) -> Result<Self, StateError> {
        let db = redb::Database::open(path)?;
        let version = {
            let txn = db.begin_read()?;
            txn.open_table(SCHEMA_VERSION)
                .ok()
                .and_then(|table| table.get("version").ok())
                .and_then(|opt| opt.map(|g| g.value()))
                .unwrap_or(0)
        };
        if version != STATE_SCHEMA_VERSION {
            return Err(StateError::Schema {
                expected: STATE_SCHEMA_VERSION,
                found: version,
            });
        }
        Ok(Self {
            db,
            path: path.to_path_buf(),
            read_only: true,
        })
    }

    /// Path of the backing database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether writes are rejected (STATE-004 no-write path).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Persist `event`, preserving any existing `first_seen_at` and stamping
    /// `last_seen_at = now`. Returns the stored event as written.
    pub fn store_event(&self, event: &Event, now: DateTime<Utc>) -> Result<Event, StateError> {
        if self.read_only {
            return Err(StateError::ReadOnly);
        }
        let mut stored = event.clone();
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(EVENTS)?;
            let key = event.id.0.as_str();
            let prev_first = match table.get(key)? {
                Some(g) => {
                    let prev: Event = serde_json::from_slice(g.value())?;
                    prev.first_seen_at
                }
                None => None,
            };
            stored.first_seen_at = Some(prev_first.unwrap_or(now));
            stored.last_seen_at = Some(now);
            let bytes = serde_json::to_vec(&stored)?;
            table.insert(key, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(stored)
    }

    /// Atomically compare-and-store a scan's events (§23). Within ONE write
    /// transaction: reads all previous events, runs change detection, upserts
    /// each current event preserving existing `first_seen_at` and stamping
    /// `last_seen_at = now`. Returns the stored events and the change records.
    ///
    /// This is the canonical scan-path write primitive. It makes read-then-
    /// write atomic so the previous-event ordering required by change
    /// detection cannot be violated. Unlike [`Repository::store_event`], the
    /// previous events are deserialized exactly once — for change detection —
    /// and the resulting in-memory map is reused for `first_seen_at` lookup,
    /// avoiding the per-upsert full re-deserialization that `store_event`
    /// performs (ST-2).
    ///
    /// Memory: both the previous and current event sets are materialized
    /// simultaneously (peak ≈ 2× corpus) because `detect_changes` takes
    /// `&[Event]` slices. Adequate for v0.1 batch sizes (low thousands); a
    /// future iterator-based signature would bound peak memory.
    /// The full scored `Event` is persisted verbatim — `score` /
    /// `score_components` / `rank_reasons` are transient ranking output that
    /// is recomputed each scan, so persisting them is redundant; a projected
    /// fingerprint struct would be leaner but is deferred to avoid a schema
    /// migration before v0.1.
    pub fn store_scan(
        &self,
        events: &[Event],
        now: DateTime<Utc>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        self.store_scan_bundle(events, &[], now)
    }

    /// Atomically compare-and-store a scan's events, change log, and source
    /// health in ONE redb transaction (ADR-0011 §6). Also purges expired
    /// tombstones, health records, and change records (> [`RETENTION_DAYS`])
    /// in the same transaction.
    ///
    /// This is the canonical scan-path write primitive. It supersedes
    /// [`Repository::store_scan`] (which delegates here with an empty health
    /// slice). Health records are written here in the scan's atomic
    /// transaction under composite keys `"{source}\x00{recorded_at}"`.
    ///
    /// Within the transaction:
    /// 1. Reads all previous events and tombstones.
    /// 2. Runs `detect_changes` → `Vec<ChangeRecord>`.
    /// 3. Upserts each current event, preserving `first_seen_at` (INV-1) and
    ///    restoring from tombstones within the retention window (INV-2).
    /// 4. Prunes absent events: writes tombstones, removes event rows (INV-4).
    /// 5. Appends each `ChangeRecord` to `CHANGE_LOG` (R9-H08).
    /// 6. Appends each `source_health` record to `SOURCE_HEALTH` under
    ///    composite key `"{source}\x00{recorded_at}"` (R9-M06/B06). Defensive:
    ///    stamps `recorded_at = now` if the caller left it `None`.
    /// 7. Purges expired rows from `CANCELLED_EVENTS`, `SOURCE_HEALTH`, and
    ///    `CHANGE_LOG` (all older than `now - RETENTION_DAYS`).
    ///
    /// A failure rolls back the entire transaction (TXN-2). The scan path
    /// surfaces this as exit 5 (CLI-21). The `--no-state` path opens
    /// read-only and never calls this method (RO-1).
    pub fn store_scan_bundle(
        &self,
        events: &[Event],
        source_health: &[SourceHealth],
        now: DateTime<Utc>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        if self.read_only {
            return Err(StateError::ReadOnly);
        }
        let txn = self.db.begin_write()?;
        let (stored, changes) = {
            let mut table = txn.open_table(EVENTS)?;
            let mut tombstones = txn.open_table(CANCELLED_EVENTS)?;
            let mut health_table = txn.open_table(SOURCE_HEALTH)?;
            let mut change_log = txn.open_table(CHANGE_LOG)?;
            // Read all previous events once — used for both change detection
            // and `first_seen_at` preservation (ST-2: no per-upsert re-deser).
            let mut prev_events: Vec<Event> = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                prev_events.push(serde_json::from_slice(value.value())?);
            }
            let prev_first_seen: HashMap<&str, Option<DateTime<Utc>>> = prev_events
                .iter()
                .map(|e| (e.id.0.as_str(), e.first_seen_at))
                .collect();
            // ST-16: read tombstones so a reappearing event restores its
            // original `first_seen_at` instead of being treated as brand-new.
            // Tombstones past the retention window are treated as if purged:
            // a reappear past the window is a genuinely new event (INV-3).
            let retention_cutoff = now - chrono::Duration::days(RETENTION_DAYS);
            let mut tombstone_first_seen: HashMap<String, DateTime<Utc>> = HashMap::new();
            let mut expired_tombstone_keys: Vec<String> = Vec::new();
            for entry in tombstones.iter()? {
                let (key, value) = entry?;
                let t: CancelledEventTombstone = serde_json::from_slice(value.value())?;
                if t.cancelled_at < retention_cutoff {
                    expired_tombstone_keys.push(key.value().to_string());
                } else {
                    tombstone_first_seen.insert(key.value().to_string(), t.first_seen_at);
                }
            }
            let changes = detect_changes(&prev_events, events, now);
            let current_ids: std::collections::HashSet<&str> =
                events.iter().map(|e| e.id.0.as_str()).collect();
            let mut stored = Vec::with_capacity(events.len());
            for event in events {
                let mut s = event.clone();
                let prev_first = prev_first_seen.get(event.id.0.as_str()).copied().flatten();
                // ST-16 / INV-2: if the event was previously cancelled (a
                // tombstone exists), restore its `first_seen_at` and remove
                // the tombstone — the event is no longer cancelled.
                let restored_first = tombstone_first_seen.get(&event.id.0).copied();
                let first_seen = prev_first.or(restored_first).unwrap_or(now);
                if restored_first.is_some() {
                    tombstones.remove(event.id.0.as_str())?;
                }
                s.first_seen_at = Some(first_seen);
                s.last_seen_at = Some(now);
                let bytes = serde_json::to_vec(&s)?;
                table.insert(event.id.0.as_str(), bytes.as_slice())?;
                stored.push(s);
            }
            // ST M-1 / INV-4: prune events that were in the previous scan but
            // are absent from the current one (the cancelled set). Write a
            // tombstone preserving `first_seen_at` so a future reappearance
            // restores it instead of resetting to the reappearance scan time.
            for prev in &prev_events {
                if !current_ids.contains(prev.id.0.as_str()) {
                    if let Some(first_seen) = prev.first_seen_at {
                        let tombstone = CancelledEventTombstone {
                            first_seen_at: first_seen,
                            cancelled_at: now,
                        };
                        let bytes = serde_json::to_vec(&tombstone)?;
                        tombstones.insert(prev.id.0.as_str(), bytes.as_slice())?;
                    }
                    table.remove(prev.id.0.as_str())?;
                }
            }
            // INV-4: purge expired tombstones collected during the read pass.
            for key in expired_tombstone_keys {
                tombstones.remove(key.as_str())?;
            }

            // ADR-0011 §3 (R9-H08): persist change records to CHANGE_LOG so
            // media history and change signals survive a restart (§65).
            for record in &changes {
                let key = format!(
                    "{}\u{0}{}\u{0}{}",
                    record.detected_at.to_rfc3339(),
                    record.event_id.0,
                    record.kind.as_str()
                );
                let bytes = serde_json::to_vec(record)?;
                change_log.insert(key.as_str(), bytes.as_slice())?;
            }

            // ADR-0011 §1/§2 (R9-M06/B06): persist source-health observations
            // to SOURCE_HEALTH under composite key for per-scan history.
            // Defensive: stamp recorded_at = now if the caller left it None.
            for h in source_health {
                let (ts, bytes) = match h.recorded_at {
                    Some(existing) => (existing, serde_json::to_vec(h)?),
                    None => {
                        let mut stamped = h.clone();
                        stamped.recorded_at = Some(now);
                        (now, serde_json::to_vec(&stamped)?)
                    }
                };
                let key = format!("{}\u{0}{}", h.source, ts.to_rfc3339());
                health_table.insert(key.as_str(), bytes.as_slice())?;
            }

            // ADR-0011 §7: purge expired SOURCE_HEALTH and CHANGE_LOG records
            // (older than RETENTION_DAYS). Same transaction as the writes.
            // Key-based expiry: the composite keys encode the timestamp, so we
            // parse it from the key string instead of deserializing the full
            // value — O(n) iteration but zero serde cost per row, and a
            // corrupt value can't crash the purge.
            let cutoff_rfc3339 = retention_cutoff.to_rfc3339();
            let mut expired_health_keys: Vec<String> = Vec::new();
            for entry in health_table.iter()? {
                let (key, _value) = entry?;
                let key_str = key.value();
                if let Some(idx) = key_str.find('\u{0}') {
                    let ts = &key_str[idx + 1..];
                    if ts < cutoff_rfc3339.as_str() {
                        expired_health_keys.push(key_str.to_string());
                    }
                }
            }
            for key in expired_health_keys {
                health_table.remove(key.as_str())?;
            }

            // CHANGE_LOG key starts with `{detected_at_rfc3339}\x00...`, so
            // range(..cutoff) yields exactly the expired records — O(log n +
            // expired), zero deserialization.
            let mut expired_change_keys: Vec<String> = Vec::new();
            for entry in change_log.range(..cutoff_rfc3339.as_str())? {
                let (key, _value) = entry?;
                expired_change_keys.push(key.value().to_string());
            }
            for key in expired_change_keys {
                change_log.remove(key.as_str())?;
            }

            (stored, changes)
        };
        txn.commit()?;
        Ok((stored, changes))
    }

    /// Retrieve a single event by id, or `None` if absent.
    pub fn get_event(&self, id: &EventId) -> Result<Option<Event>, StateError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(EVENTS)?;
        match table.get(id.0.as_str())? {
            Some(g) => Ok(Some(serde_json::from_slice(g.value())?)),
            None => Ok(None),
        }
    }

    /// List all stored events. Order is by key (event id) ascending — stable
    /// across calls and independent of insertion order.
    pub fn list_events(&self) -> Result<Vec<Event>, StateError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(EVENTS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            out.push(serde_json::from_slice(value.value())?);
        }
        Ok(out)
    }

    /// List source-health history for `source`, ordered oldest-to-newest by
    /// `recorded_at` (ADR-0011 §2). Uses a prefix range scan
    /// `"{source}\x00".."{source}\x01"` so only this source's rows are visited
    /// — O(log n + k) instead of a full-table scan with prefix filtering.
    /// Legacy keys (bare source id, pre-v3) fall outside the range and are
    /// skipped.
    pub fn list_source_health(&self, source: &str) -> Result<Vec<SourceHealth>, StateError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(SOURCE_HEALTH)?;
        let start = format!("{source}\u{0}");
        let end = format!("{source}\u{1}");
        let mut out = Vec::new();
        for entry in table.range(start.as_str()..end.as_str())? {
            let (_, value) = entry?;
            out.push(serde_json::from_slice(value.value())?);
        }
        Ok(out)
    }

    /// List change records since `since` (ADR-0011 §3, R9-H08). Ordered
    /// oldest-to-newest by `detected_at` (composite key lexicographic sort).
    pub fn list_changes(&self, since: DateTime<Utc>) -> Result<Vec<ChangeRecord>, StateError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CHANGE_LOG)?;
        let start = since.to_rfc3339();
        let mut out = Vec::new();
        for entry in table.range(start.as_str()..)? {
            let (_, value) = entry?;
            out.push(serde_json::from_slice(value.value())?);
        }
        Ok(out)
    }

    /// The persisted schema version.
    pub fn schema_version(&self) -> Result<u32, StateError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(SCHEMA_VERSION)?;
        Ok(table.get("version")?.map(|g| g.value()).unwrap_or(0))
    }
}
