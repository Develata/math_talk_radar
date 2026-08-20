//! Schema migrations (§65, ADR-0011). Transactional: a failure leaves no
//! half-migrated state. `first_seen` and media history must not be silently
//! lost. The v1 migration creates the base tables (`events`, `source_health`)
//! and writes the schema-version row. The v2 migration (ST-16) adds the
//! `cancelled_events` tombstone table; existing v1 databases are upgraded in
//! place (additive — no data is rewritten, the new table is created by the
//! `open_table` call at the end of [`run_migrations`]). The v3 migration
//! (ADR-0011) adds the `change_log` table and re-keys any legacy
//! `source_health` rows from bare source id to composite
//! `"{source}\x00{recorded_at}"`; on real v2 databases the table is empty
//! (the scan path never wrote it), so the re-key is defensive.

use redb::{Database, ReadableTable};

use crate::schema::{
    CANCELLED_EVENTS, CHANGE_LOG, EVENTS, SCHEMA_VERSION, SOURCE_HEALTH, STATE_SCHEMA_VERSION,
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
    let found_version = {
        let vtable = txn.open_table(SCHEMA_VERSION)?;
        vtable.get("version")?.map(|g| g.value())
    };
    // Backward: newer DB, older binary — refuse.
    if let Some(v) = found_version
        && v > STATE_SCHEMA_VERSION
    {
        return Err(MigrateError::UnsupportedVersion {
            found: v,
            expected: STATE_SCHEMA_VERSION,
        });
    }

    let _ = txn.open_table(EVENTS)?;
    let _ = txn.open_table(CANCELLED_EVENTS)?;
    let _ = txn.open_table(CHANGE_LOG)?;

    // ADR-0011 v3: re-key legacy SOURCE_HEALTH rows from bare source id to
    // composite "{source}\x00{recorded_at}". On real v2 databases the table
    // is empty (scan path never wrote it); this is defensive — if legacy
    // rows exist, stamp recorded_at = migration_time and re-insert.
    //
    // R3-P1-02: fail-closed on malformed legacy rows. A deserialization or
    // storage error aborts the migration without bumping the version — the
    // transaction is dropped (rolled back), so the DB stays at its previous
    // version. The previous code silently skipped malformed rows and still
    // committed v3, making them unreachable (the v3 read path skips
    // non-composite keys).
    let migration_time = chrono::Utc::now();
    {
        let mut health = txn.open_table(SOURCE_HEALTH)?;
        let legacy_rows: Vec<(String, Vec<u8>)> = {
            let mut rows = Vec::new();
            for entry in health.iter()? {
                let (k, v) = entry?;
                let key_str = k.value();
                if !key_str.contains('\u{0}') {
                    rows.push((key_str.to_string(), v.value().to_vec()));
                }
            }
            rows
        };
        for (source_id, bytes) in legacy_rows {
            let mut h: radar_core::SourceHealth =
                serde_json::from_slice(&bytes).map_err(|e| MigrateError::MalformedLegacyRow {
                    source_id: source_id.clone(),
                    error: e.to_string(),
                })?;
            if h.recorded_at.is_none() {
                h.recorded_at = Some(migration_time);
            }
            let composite = format!(
                "{}\u{0}{}",
                source_id,
                h.recorded_at.map(|t| t.to_rfc3339()).unwrap_or_default()
            );
            let new_bytes =
                serde_json::to_vec(&h).map_err(|e| MigrateError::MalformedLegacyRow {
                    source_id: source_id.clone(),
                    error: format!("re-serialize: {e}"),
                })?;
            health.insert(composite.as_str(), new_bytes.as_slice())?;
            health.remove(source_id.as_str())?;
        }
    }

    // Bump version row to current (forward migration or fresh DB).
    {
        let mut vtable = txn.open_table(SCHEMA_VERSION)?;
        vtable.insert("version", STATE_SCHEMA_VERSION)?;
    }
    txn.commit()?;
    Ok(STATE_SCHEMA_VERSION)
}

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("state schema version mismatch: expected {expected}, found {found}")]
    UnsupportedVersion { expected: u32, found: u32 },
    #[error("malformed legacy source_health row for {source_id}: {error}")]
    MalformedLegacyRow { source_id: String, error: String },
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
