//! Schema migrations (§65). Transactional: a failure leaves no half-migrated
//! state. `first_seen` and media history must not be silently lost. The v1
//! migration creates the base tables (`events`, `source_health`) and writes
//! the schema-version row. The v2 migration (ST-16) adds the
//! `cancelled_events` tombstone table; existing v1 databases are upgraded in
//! place (additive — no data is rewritten, the new table is created by the
//! `open_table` call at the end of [`run_migrations`]).

use redb::{Database, ReadableTable};

use crate::schema::{
    CANCELLED_EVENTS, EVENTS, SCHEMA_VERSION, SOURCE_HEALTH, STATE_SCHEMA_VERSION,
};

/// Run migrations against `db` and return the active schema version. All
/// table creation happens inside a single write transaction so a failure
/// rolls back cleanly. Opening an existing table is a no-op (redb creates it
/// if missing).
///
/// # Forward vs. backward
///
/// - **Forward** (`found < current`): the DB was created by an older binary.
///   Migrations run additively — the version row is bumped and any new tables
///   are created by the `open_table` calls below. Existing data is preserved.
/// - **Backward** (`found > current`): the DB was created by a newer binary.
///   Refused — a downgrade could silently drop columns/tables the newer schema
///   relies on.
/// - **Fresh** (`None`): no version row exists; write the current version and
///   create all tables.
pub fn run_migrations(db: &Database) -> Result<u32, MigrateError> {
    let txn = db.begin_write()?;
    let version = {
        let mut vtable = txn.open_table(SCHEMA_VERSION)?;
        let existing = vtable.get("version")?.map(|g| g.value());
        match existing {
            // Backward: newer DB, older binary — refuse.
            Some(v) if v > STATE_SCHEMA_VERSION => {
                return Err(MigrateError::UnsupportedVersion {
                    found: v,
                    expected: STATE_SCHEMA_VERSION,
                });
            }
            // Forward: older DB — bump the version row; the open_table calls
            // below create any tables missing from the older schema.
            Some(v) if v < STATE_SCHEMA_VERSION => {
                vtable.insert("version", STATE_SCHEMA_VERSION)?;
                STATE_SCHEMA_VERSION
            }
            // Already current — no-op.
            Some(v) => v,
            // Fresh DB — write current version, all tables created below.
            None => {
                vtable.insert("version", STATE_SCHEMA_VERSION)?;
                STATE_SCHEMA_VERSION
            }
        }
    };
    let _ = txn.open_table(EVENTS)?;
    let _ = txn.open_table(SOURCE_HEALTH)?;
    // ST-16: v2 adds the cancelled-events tombstone table. On a v1 database
    // this creates it (no data to migrate); on a v2 database it's a no-op.
    let _ = txn.open_table(CANCELLED_EVENTS)?;
    txn.commit()?;
    Ok(version)
}

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("state schema version mismatch: expected {expected}, found {found}")]
    UnsupportedVersion { expected: u32, found: u32 },
    #[error("state migration backend error: {0}")]
    Backend(Box<redb::Error>),
}

// Manual From impls so `?` converts each redb-specific error into the single
// boxed backend variant. Boxed to keep the Err variant small (some redb errors
// embed a ReadTransaction, which is large — clippy::result_large_err).
impl From<redb::DatabaseError> for MigrateError {
    fn from(e: redb::DatabaseError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::TransactionError> for MigrateError {
    fn from(e: redb::TransactionError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::TableError> for MigrateError {
    fn from(e: redb::TableError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::CommitError> for MigrateError {
    fn from(e: redb::CommitError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
impl From<redb::StorageError> for MigrateError {
    fn from(e: redb::StorageError) -> Self {
        Self::Backend(Box::new(e.into()))
    }
}
