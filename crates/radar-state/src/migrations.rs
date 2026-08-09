//! Schema migrations (§65). Transactional: a failure leaves no half-migrated
//! state. `first_seen` and media history must not be silently lost. The v1
//! migration creates all tables and writes the schema-version row.

use redb::{Database, ReadableTable};

use crate::schema::{EVENTS, SCHEMA_VERSION, SOURCE_HEALTH, STATE_SCHEMA_VERSION};

/// Run migrations against `db` and return the active schema version. All
/// table creation happens inside a single write transaction so a failure
/// rolls back cleanly. Opening an existing table is a no-op.
pub fn run_migrations(db: &Database) -> Result<u32, MigrateError> {
    let txn = db.begin_write()?;
    let version = {
        let mut vtable = txn.open_table(SCHEMA_VERSION)?;
        let existing = vtable.get("version")?.map(|g| g.value());
        match existing {
            Some(v) => v,
            None => {
                vtable.insert("version", STATE_SCHEMA_VERSION)?;
                STATE_SCHEMA_VERSION
            }
        }
    };
    // Open-for-create the domain tables so they exist after v1. Opening under a
    // write transaction creates the table if absent and is a no-op otherwise.
    let _ = txn.open_table(EVENTS)?;
    let _ = txn.open_table(SOURCE_HEALTH)?;
    txn.commit()?;

    if version != STATE_SCHEMA_VERSION {
        return Err(MigrateError::UnsupportedVersion {
            found: version,
            expected: STATE_SCHEMA_VERSION,
        });
    }
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
