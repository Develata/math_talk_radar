//! Install manifest (§36). Records the official install path, method, and
//! version so uninstall can identify app-owned paths and protect unmanaged
//! (e.g. `cargo run`) binaries.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MANIFEST_FILENAME: &str = "install-manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallManifest {
    pub binary_path: PathBuf,
    pub install_method: String,
    pub installed_version: String,
}

impl InstallManifest {
    pub fn new(
        binary_path: PathBuf,
        install_method: impl Into<String>,
        installed_version: impl Into<String>,
    ) -> Self {
        Self {
            binary_path,
            install_method: install_method.into(),
            installed_version: installed_version.into(),
        }
    }

    pub fn manifest_path(data_dir: &Path) -> PathBuf {
        data_dir.join(MANIFEST_FILENAME)
    }

    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        if !data_dir.exists() {
            std::fs::create_dir_all(data_dir)?;
        }
        let path = Self::manifest_path(data_dir);
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&path, json)?;
        Ok(())
    }
}

/// Load the manifest from `data_dir`, if present. Returns `None` if the file
/// is missing or unparseable (treated as unmanaged install).
pub fn load(data_dir: &Path) -> Option<InstallManifest> {
    let path = InstallManifest::manifest_path(data_dir);
    let contents = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&contents).ok()
}
