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

    // H12: serialize against `update`. Without this lock, a concurrent
    // `update` could be mid-rename while `uninstall` deletes the binary and
    // rollback copy — update's post-replace self-test would then run against
    // a deleted path, or its `rename` would fail into a half-deleted tree.
    // The lock is acquired before any path resolution or deletion. `--dry-run`
    // also acquires it so a dry-run cannot inspect a tree that a live update
    // is mutating underneath it. The lock-failure error is remapped to an
    // uninstall-fatal code (exit 11) so the exit code matches the command
    // the user ran, not the shared lock's origin subsystem.
    let _lock = crate::lifecycle::update::acquire_update_lock().map_err(|e| {
        CliError::uninstall(format!("could not acquire update lock: {}", e.message))
    })?;

    let data_dir = paths::data_dir();
    let config_dir = paths::config_dir();
    let cache_dir = paths::cache_dir();

    // B08: refuse if any two of config/cache/data canonicalize to the same
    // path. A misconfigured XDG setup (e.g. XDG_CONFIG_HOME == XDG_DATA_HOME)
    // would make --keep-data delete the data dir via the config-deletion
    // branch, or make --purge delete the same dir twice.
    paths::detect_dir_overlap(&config_dir, &cache_dir, &data_dir).map_err(CliError::uninstall)?;

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
    let mut to_delete: Vec<(PathBuf, bool)> = vec![
        (binary.clone(), true),
        (config_dir.clone(), false),
        (cache_dir.clone(), false),
    ];
    if !preserve_data {
        to_delete.push((data_dir.clone(), false));
    } else {
        let manifest_path = manifest::InstallManifest::manifest_path(&data_dir);
        if manifest_path.exists() {
            to_delete.push((manifest_path, false));
        }
    }

    if let Some(parent) = binary.parent()
        && let Ok(entries) = std::fs::read_dir(parent)
    {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".math_talk_radar.update.")
                || name.starts_with(".math_talk_radar.rollback")
            {
                to_delete.push((entry.path(), false));
            }
        }
    }

    if args.dry_run {
        let mut plan = String::from("uninstall plan (dry-run, nothing will be deleted):\n");
        for (p, is_binary) in &to_delete {
            let status = if p.exists() { "exists" } else { "missing" };
            let kind = if *is_binary { "binary" } else { "path" };
            plan.push_str(&format!("  delete [{status}] {kind}: {}\n", p.display()));
        }
        if preserve_data {
            plan.push_str(&format!("  preserve: {}\n", data_dir.display()));
        }
        return Ok(plan);
    }

    let mut deleted = Vec::new();
    for (p, is_binary) in &to_delete {
        if !p.exists() {
            continue;
        }
        delete_path(p, *is_binary)?;
        deleted.push(p.clone());
    }

    let mut result = format!("uninstalled: deleted {} path(s)\n", deleted.len());
    if preserve_data {
        result.push_str(&format!("preserved data: {}\n", data_dir.display()));
    }
    Ok(result)
}

/// B2-1: `is_binary=true` forces `remove_file` — a binary path must NEVER be
/// recursively deleted even if it somehow points at a directory. Only the
/// app's own config/cache/data dirs use `remove_dir_all`.
///
/// B07: safe_canonicalize's result must drive the actual deletion. The
/// previous code validated the canonical path but then deleted the original
/// (possibly symlink-bearing) path — a TOCTOU where an attacker swaps the
/// path for a symlink between canonicalize and remove. Deleting the
/// canonical path avoids following any symlink: it is the resolved, real
/// location. The `is_binary`/`is_dir` checks use `symlink_metadata` on the
/// canonical path (which never follows symlinks) so the file-type decision
/// matches the path being deleted.
fn delete_path(path: &Path, is_binary: bool) -> Result<(), CliError> {
    let canonical = paths::safe_canonicalize(path).map_err(CliError::uninstall)?;
    let meta = std::fs::symlink_metadata(&canonical)
        .map_err(|e| CliError::uninstall(format!("stat {}: {e}", canonical.display())))?;
    if is_binary {
        if meta.is_dir() {
            return Err(CliError::uninstall(format!(
                "refusing to delete binary path that is a directory: {}",
                canonical.display()
            )));
        }
        std::fs::remove_file(&canonical)
            .map_err(|e| CliError::uninstall(format!("delete {}: {e}", canonical.display())))?;
    } else if meta.is_dir() {
        std::fs::remove_dir_all(&canonical)
            .map_err(|e| CliError::uninstall(format!("delete {}: {e}", canonical.display())))?;
    } else {
        std::fs::remove_file(&canonical)
            .map_err(|e| CliError::uninstall(format!("delete {}: {e}", canonical.display())))?;
    }
    Ok(())
}
