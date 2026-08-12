//! Uninstall (§35). Deletes only known app-owned paths. No `rm -rf`, no
//! symlink following, no `$HOME` deletion. Unmanaged binaries (no manifest,
//! under `target/`) are protected without `--force-unmanaged`.
use std::path::{Path, PathBuf};

use crate::cli::UninstallArgs;
use crate::lifecycle::manifest;
use crate::lifecycle::paths;
use crate::runtime::CliError;

pub async fn run(args: UninstallArgs) -> Result<String, CliError> {
    if !args.keep_data && !args.purge {
        return Err(CliError::uninstall("requires --keep-data or --purge"));
    }
    if args.keep_data && args.purge {
        return Err(CliError::uninstall(
            "--keep-data and --purge are mutually exclusive",
        ));
    }
    if !args.dry_run && !args.yes {
        return Err(CliError::uninstall("noninteractive mode requires --yes"));
    }

    let data_dir = paths::data_dir();
    let config_dir = paths::config_dir();
    let cache_dir = paths::cache_dir();
    let binary = paths::binary_path(&data_dir)
        .ok_or_else(|| CliError::uninstall("cannot resolve binary path"))?;

    // §36: protect dev binaries. A stale manifest (recorded path gone) makes
    // binary_path() fall back to current_exe(), which may be a target/ dev
    // binary — so the guard keys on whether the manifest actually manages the
    // resolved path, not on whether a manifest file merely exists.
    let manifest = manifest::load(&data_dir);
    let managed_by_manifest = manifest
        .as_ref()
        .map(|m| m.binary_path == binary)
        .unwrap_or(false);
    if !managed_by_manifest && paths::is_unmanaged_binary(&binary) && !args.force_unmanaged {
        return Err(CliError::uninstall(format!(
            "refusing to delete unmanaged binary: {} (use --force-unmanaged to override)",
            binary.display()
        )));
    }

    let preserve_data = args.keep_data;
    let mut to_delete: Vec<PathBuf> = vec![binary.clone(), config_dir.clone(), cache_dir.clone()];
    if !preserve_data {
        to_delete.push(data_dir.clone());
    } else {
        // keep-data still removes the install manifest (§35.3 "install/update metadata")
        let manifest_path = manifest::InstallManifest::manifest_path(&data_dir);
        if manifest_path.exists() {
            to_delete.push(manifest_path);
        }
    }

    // Clean up stale temp files near the binary (§34.3)
    if let Some(parent) = binary.parent()
        && let Ok(entries) = std::fs::read_dir(parent)
    {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".math_talk_radar.update.")
                || name.starts_with(".math_talk_radar.rollback")
            {
                to_delete.push(entry.path());
            }
        }
    }

    if args.dry_run {
        let mut plan = String::from("uninstall plan (dry-run, nothing will be deleted):\n");
        for p in &to_delete {
            let status = if p.exists() { "exists" } else { "missing" };
            plan.push_str(&format!("  delete [{status}]: {}\n", p.display()));
        }
        if preserve_data {
            plan.push_str(&format!("  preserve: {}\n", data_dir.display()));
        }
        return Ok(plan);
    }

    let mut deleted = Vec::new();
    for p in &to_delete {
        if !p.exists() {
            continue;
        }
        delete_path(p)?;
        deleted.push(p.clone());
    }

    let mut result = format!("uninstalled: deleted {} path(s)\n", deleted.len());
    if preserve_data {
        result.push_str(&format!("preserved data: {}\n", data_dir.display()));
    }
    Ok(result)
}

fn delete_path(path: &Path) -> Result<(), CliError> {
    // Canonicalize validates safety (rejects /, $HOME, empty) but we delete the
    // original path: symlink_metadata does not follow symlinks, so a symlink is
    // unlinked rather than its target (§35.5).
    paths::safe_canonicalize(path).map_err(CliError::uninstall)?;
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| CliError::uninstall(format!("stat {}: {e}", path.display())))?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
            .map_err(|e| CliError::uninstall(format!("delete {}: {e}", path.display())))?;
    } else {
        std::fs::remove_file(path)
            .map_err(|e| CliError::uninstall(format!("delete {}: {e}", path.display())))?;
    }
    Ok(())
}
