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

use crate::changes::{ChangeRecord, detect_changes};
use crate::migrations::run_migrations;
use crate::schema::{EVENTS, SCHEMA_VERSION, SOURCE_HEALTH, STATE_SCHEMA_VERSION};

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
    /// store_source_health) return [`StateError::ReadOnly`]. The database file
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
        if self.read_only {
            return Err(StateError::ReadOnly);
        }
        let txn = self.db.begin_write()?;
        let (stored, changes) = {
            let mut table = txn.open_table(EVENTS)?;
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
            let changes = detect_changes(&prev_events, events, now);
            let current_ids: std::collections::HashSet<&str> =
                events.iter().map(|e| e.id.0.as_str()).collect();
            let mut stored = Vec::with_capacity(events.len());
            for event in events {
                let mut s = event.clone();
                let prev_first = prev_first_seen.get(event.id.0.as_str()).copied().flatten();
                s.first_seen_at = Some(prev_first.unwrap_or(now));
                s.last_seen_at = Some(now);
                let bytes = serde_json::to_vec(&s)?;
                table.insert(event.id.0.as_str(), bytes.as_slice())?;
                stored.push(s);
            }
            // ST M-1: prune events that were in the previous scan but are absent
            // from the current one (the cancelled set). Without this, cancelled
            // events stay in the table forever and detect_changes re-emits
            // EventCancelled for them on every subsequent scan. Deleting here
            // keeps the DB bounded and makes EventCancelled a one-shot signal.
            for prev in &prev_events {
                if !current_ids.contains(prev.id.0.as_str()) {
                    table.remove(prev.id.0.as_str())?;
                }
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

    /// Persist a source-health record, overwriting any previous entry for the
    /// same source id.
    pub fn store_source_health(&self, health: &SourceHealth) -> Result<(), StateError> {
        if self.read_only {
            return Err(StateError::ReadOnly);
        }
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(SOURCE_HEALTH)?;
            let bytes = serde_json::to_vec(health)?;
            table.insert(health.source.as_str(), bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// The persisted schema version.
    pub fn schema_version(&self) -> Result<u32, StateError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(SCHEMA_VERSION)?;
        Ok(table.get("version")?.map(|g| g.value()).unwrap_or(0))
    }
}
