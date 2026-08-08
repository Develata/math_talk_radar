//! State repository (§22). The `redb`-backed implementation (tables for event
//! fingerprints, first/last seen, talk/media fingerprints, source health,
//! change-detection state) lands in M3. This stub establishes the open surface.
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("state io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("state schema mismatch: expected {expected}, found {found}")]
    Schema { expected: u32, found: u32 },
}

/// Persistent state repository. `redb` integration lands in M3.
#[derive(Debug)]
pub struct Repository {
    path: PathBuf,
}

impl Repository {
    pub fn open(path: &Path) -> Result<Self, StateError> {
        // M3: open/create the redb database, run migrations, verify schema.
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
