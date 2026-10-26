//! Explicit engine-format migration. The source is a retained rollback copy;
//! only a private candidate is upgraded and an existing destination is never
//! overwritten (ADR-0018). Application schema migrations remain transactional.

use std::fs::File;
use std::path::Path;

use redb::{ReadableDatabase, ReadableTable};

use crate::schema::{CANCELLED_EVENTS, CHANGE_LOG, EVENTS, SCHEMA_VERSION, SOURCE_HEALTH};
use crate::{CancelledEventTombstone, ChangeRecord, Repository, STATE_SCHEMA_VERSION, StateError};

/// Migrate an existing database into a new path, retaining `source` unchanged.
/// Stop other radar processes first. The source lock is held through copying,
/// validation and publication. A failed candidate is removed by its temp-file
/// guard; no destination becomes visible until validation succeeds.
pub fn migrate_copy(source: &Path, destination: &Path) -> Result<(), StateError> {
    let mut input = File::open(source)?;
    if !input.metadata()?.is_file() {
        return Err(StateError::Migration(
            "source must be a regular file".into(),
        ));
    }
    input.try_lock().map_err(|error| {
        StateError::Migration(format!(
            "cannot lock source; stop other radar processes: {error}"
        ))
    })?;
    // A broken symlink also counts as an existing destination. Publication
    // uses no-clobber semantics as well, to close the check/create race.
    match destination.symlink_metadata() {
        Ok(_) => return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut candidate = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut input, candidate.as_file_mut())?;
    candidate.as_file().sync_all()?;

    upgrade_engine(candidate.path())?;
    // Run application migrations only on the candidate; failures cannot leave
    // the source in a newer engine format with an older application schema.
    drop(Repository::open(candidate.path())?);
    validate(candidate.path())?;
    candidate.as_file().sync_all()?;
    candidate
        .persist_noclobber(destination)
        .map_err(|error| StateError::Io(error.error))?;
    #[cfg(unix)]
    File::open(parent).and_then(|directory| directory.sync_all()).map_err(|error| {
        StateError::Migration(format!("destination was published but its directory could not be synced: {error}; source is unchanged"))
    })?;
    Ok(())
}

fn upgrade_engine(path: &Path) -> Result<(), StateError> {
    // Never feed a file written by redb 4 into the legacy engine. Reverse
    // compatibility is not guaranteed even when the format number is v3.
    match redb::Database::open(path) {
        Ok(mut db) => {
            db.check_integrity()?;
            let read = db.begin_read()?;
            let version = read
                .open_table(SCHEMA_VERSION)?
                .get("version")?
                .map(|value| value.value())
                .unwrap_or(0);
            supported_version(version)?;
        }
        Err(redb::DatabaseError::UpgradeRequired(2)) => {
            let mut legacy = redb_legacy::Database::open(path).map_err(legacy_error)?;
            legacy.check_integrity().map_err(legacy_error)?;
            let version = {
                let read = legacy.begin_read().map_err(legacy_error)?;
                let table = read
                    .open_table(redb_legacy::TableDefinition::<&str, u32>::new(
                        "schema_version",
                    ))
                    .map_err(legacy_error)?;
                table
                    .get("version")
                    .map_err(legacy_error)?
                    .map(|value| value.value())
                    .unwrap_or(0)
            };
            supported_version(version)?;
            legacy.upgrade().map_err(legacy_error)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn supported_version(version: u32) -> Result<(), StateError> {
    if !(1..=STATE_SCHEMA_VERSION).contains(&version) {
        return Err(StateError::Schema {
            expected: STATE_SCHEMA_VERSION,
            found: version,
        });
    }
    Ok(())
}

fn legacy_error(error: impl std::fmt::Display) -> StateError {
    StateError::Migration(format!("legacy database: {error}"))
}

fn validate(path: &Path) -> Result<(), StateError> {
    let db = redb::ReadOnlyDatabase::open(path)?;
    let read = db.begin_read()?;
    let version = read
        .open_table(SCHEMA_VERSION)?
        .get("version")?
        .map(|value| value.value())
        .unwrap_or(0);
    if version != STATE_SCHEMA_VERSION {
        return Err(StateError::Schema {
            expected: STATE_SCHEMA_VERSION,
            found: version,
        });
    }
    for row in read.open_table(EVENTS)?.iter()? {
        let _: radar_core::Event = serde_json::from_slice(row?.1.value())?;
    }
    for row in read.open_table(CANCELLED_EVENTS)?.iter()? {
        let _: CancelledEventTombstone = serde_json::from_slice(row?.1.value())?;
    }
    for row in read.open_table(SOURCE_HEALTH)?.iter()? {
        let _: radar_core::SourceHealth = serde_json::from_slice(row?.1.value())?;
    }
    for row in read.open_table(CHANGE_LOG)?.iter()? {
        let _: ChangeRecord = serde_json::from_slice(row?.1.value())?;
    }
    Ok(())
}
