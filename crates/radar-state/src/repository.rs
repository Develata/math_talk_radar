//! State repository (§22, ADR-0002). `redb`-backed persistence of event
//! fingerprints, first/last seen, and source health. The repository is the
//! only mutation surface for persisted state; change detection (§23) reads
//! previous state through [`Repository::list_events`] and writes the new
//! scan through [`Repository::store_event`].
//!
//! Determinism: `now` is a caller-supplied timestamp — the repository never
//! reads a wall clock, so identical inputs produce identical persisted state.
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use radar_core::{Event, EventId, SourceHealth};
use redb::ReadableTable;

use crate::migrations::run_migrations;
use crate::schema::{EVENTS, SCHEMA_VERSION, SOURCE_HEALTH};

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
            let prev_first = table
                .get(key)?
                .and_then(|g| serde_json::from_slice::<Event>(g.value()).ok())
                .and_then(|p| p.first_seen_at);
            stored.first_seen_at = Some(prev_first.unwrap_or(now));
            stored.last_seen_at = Some(now);
            let bytes = serde_json::to_vec(&stored)?;
            table.insert(key, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(stored)
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
