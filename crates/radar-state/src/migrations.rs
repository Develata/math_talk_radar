//! Schema migrations (§65, ADR-0011). Transactional: a failure leaves no
//! half-migrated state. `first_seen` and media history must not be silently
//! lost. The v1 migration creates the base tables (`events`, `source_health`)
//! and writes the schema-version row. The v2 migration (ST-16) adds the
//! `cancelled_events` tombstone table. The v3 migration (ADR-0011) adds the
//! `change_log` table and re-keys legacy `source_health` rows. The v4 migration
//! canonicalizes ordered timestamp keys and bounds change-log key size.

use std::collections::HashSet;

use redb::{Database, ReadableTable, WriteTransaction};

use crate::changes::ChangeRecord;
use crate::schema::{
    CANCELLED_EVENTS, CHANGE_LOG, EVENTS, SCHEMA_VERSION, SOURCE_HEALTH, STATE_SCHEMA_VERSION,
    change_log_key, source_health_key,
};

/// Run migrations against `db` and return the active schema version. All
/// table creation and data rewrites happen inside one write transaction, so a
/// failure rolls back cleanly.
pub fn run_migrations(db: &Database) -> Result<u32, MigrateError> {
    let txn = db.begin_write()?;
    let found_version = {
        let vtable = txn.open_table(SCHEMA_VERSION)?;
        vtable.get("version")?.map(|g| g.value())
    };

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

    // ADR-0011 v3: re-key bare SOURCE_HEALTH rows. R3-P1-02 requires every
    // iterator/storage/serde failure to abort the transaction and leave the
    // version unchanged.
    migrate_legacy_source_health(&txn)?;

    // v4: Chrono's default RFC3339 formatter uses variable fractional
    // precision, which breaks lexical time ordering within one second. Rebuild
    // both ordered tables from their serialized values using fixed-width UTC
    // timestamps. CHANGE_LOG also replaces raw detail text in the key with a
    // fixed-size deterministic digest.
    if found_version.is_some_and(|version| version < 4) {
        migrate_ordered_keys_v4(&txn)?;
    }

    {
        let mut vtable = txn.open_table(SCHEMA_VERSION)?;
        vtable.insert("version", STATE_SCHEMA_VERSION)?;
    }
    txn.commit()?;
    Ok(STATE_SCHEMA_VERSION)
}

fn migrate_legacy_source_health(txn: &WriteTransaction) -> Result<(), MigrateError> {
    let migration_time = chrono::Utc::now();
    let mut health = txn.open_table(SOURCE_HEALTH)?;
    let rows: Vec<(String, Vec<u8>)> = {
        let mut rows = Vec::new();
        for entry in health.iter()? {
            let (key, value) = entry?;
            rows.push((key.value().to_string(), value.value().to_vec()));
        }
        rows
    };

    // Validate every replacement before mutating the table. In particular, a
    // bare legacy row must never overwrite an already-composite row that maps
    // to the same source/timestamp.
    let mut occupied_keys: HashSet<String> = rows
        .iter()
        .filter(|(key, _)| key.contains('\u{0}'))
        .map(|(key, _)| key.clone())
        .collect();
    let mut replacements = Vec::new();
    for (legacy_key, bytes) in rows.into_iter().filter(|(key, _)| !key.contains('\u{0}')) {
        let mut record: radar_core::SourceHealth =
            deserialize_row("source_health", &legacy_key, &bytes)?;
        if record.recorded_at.is_none() {
            record.recorded_at = Some(migration_time);
        }
        let timestamp = record
            .recorded_at
            .ok_or_else(|| MigrateError::MalformedStateRow {
                table: "source_health",
                key: legacy_key.clone(),
                error: "recorded_at remained absent after migration stamp".to_string(),
            })?;
        let new_key = source_health_key(&record.source, timestamp);
        if !occupied_keys.insert(new_key.clone()) {
            return Err(MigrateError::KeyCollision {
                table: "source_health",
                key: new_key,
            });
        }
        let new_bytes = serialize_row("source_health", &legacy_key, &record)?;
        replacements.push((legacy_key, new_key, new_bytes));
    }

    for (legacy_key, _, _) in &replacements {
        health.remove(legacy_key.as_str())?;
    }
    for (_, new_key, bytes) in replacements {
        health.insert(new_key.as_str(), bytes.as_slice())?;
    }
    Ok(())
}

fn migrate_ordered_keys_v4(txn: &WriteTransaction) -> Result<(), MigrateError> {
    rekey_source_health_v4(txn)?;
    rekey_change_log_v4(txn)?;
    Ok(())
}

fn rekey_source_health_v4(txn: &WriteTransaction) -> Result<(), MigrateError> {
    let mut table = txn.open_table(SOURCE_HEALTH)?;
    let rows: Vec<(String, Vec<u8>)> = {
        let mut rows = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            rows.push((key.value().to_string(), value.value().to_vec()));
        }
        rows
    };

    let mut replacements = Vec::with_capacity(rows.len());
    let mut new_keys = HashSet::with_capacity(rows.len());
    for (old_key, bytes) in rows {
        let record: radar_core::SourceHealth = deserialize_row("source_health", &old_key, &bytes)?;
        let recorded_at = record
            .recorded_at
            .ok_or_else(|| MigrateError::MalformedStateRow {
                table: "source_health",
                key: old_key.clone(),
                error: "missing recorded_at in v3 history row".to_string(),
            })?;
        let new_key = source_health_key(&record.source, recorded_at);
        if !new_keys.insert(new_key.clone()) {
            return Err(MigrateError::KeyCollision {
                table: "source_health",
                key: new_key,
            });
        }
        replacements.push((old_key, new_key, bytes));
    }

    for (old_key, _, _) in &replacements {
        table.remove(old_key.as_str())?;
    }
    for (_, new_key, bytes) in replacements {
        table.insert(new_key.as_str(), bytes.as_slice())?;
    }
    Ok(())
}

fn rekey_change_log_v4(txn: &WriteTransaction) -> Result<(), MigrateError> {
    let mut table = txn.open_table(CHANGE_LOG)?;
    let rows: Vec<(String, Vec<u8>)> = {
        let mut rows = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            rows.push((key.value().to_string(), value.value().to_vec()));
        }
        rows
    };

    let mut replacements = Vec::with_capacity(rows.len());
    let mut new_keys = HashSet::with_capacity(rows.len());
    for (old_key, bytes) in rows {
        let record: ChangeRecord = deserialize_row("change_log", &old_key, &bytes)?;
        let new_key = change_log_key(&record);
        if !new_keys.insert(new_key.clone()) {
            return Err(MigrateError::KeyCollision {
                table: "change_log",
                key: new_key,
            });
        }
        replacements.push((old_key, new_key, bytes));
    }

    for (old_key, _, _) in &replacements {
        table.remove(old_key.as_str())?;
    }
    for (_, new_key, bytes) in replacements {
        table.insert(new_key.as_str(), bytes.as_slice())?;
    }
    Ok(())
}

fn deserialize_row<T: serde::de::DeserializeOwned>(
    table: &'static str,
    key: &str,
    bytes: &[u8],
) -> Result<T, MigrateError> {
    serde_json::from_slice(bytes).map_err(|error| MigrateError::MalformedStateRow {
        table,
        key: key.to_string(),
        error: error.to_string(),
    })
}

fn serialize_row<T: serde::Serialize>(
    table: &'static str,
    key: &str,
    value: &T,
) -> Result<Vec<u8>, MigrateError> {
    serde_json::to_vec(value).map_err(|error| MigrateError::MalformedStateRow {
        table,
        key: key.to_string(),
        error: format!("re-serialize: {error}"),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("state schema version mismatch: expected {expected}, found {found}")]
    UnsupportedVersion { expected: u32, found: u32 },
    #[error("malformed {table} row at key {key:?}: {error}")]
    MalformedStateRow {
        table: &'static str,
        key: String,
        error: String,
    },
    #[error("{table} migration produced duplicate key {key:?}")]
    KeyCollision { table: &'static str, key: String },
    #[error("state migration backend error: {0}")]
    Backend(Box<redb::Error>),
}

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
