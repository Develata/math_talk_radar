//! Uninstall (§35). Deletes only known app-owned paths. No `rm -rf`, no
//! symlink following, no `$HOME` deletion. Unmanaged binaries (no manifest,
//! under `target/`) are protected without `--force-unmanaged`.
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::cli::UninstallArgs;
use crate::lifecycle::manifest;
use crate::lifecycle::paths;
use crate::runtime::CliError;

pub async fn run(args: UninstallArgs) -> Result<String, CliError> {
    if args.keep_data && args.purge {
        return Err(CliError::uninstall(
            "--keep-data and --purge are mutually exclusive",
        ));
    }

    // R3-P1-04: §35.1 (TTY interactive) / §35.2 (non-TTY strict). dry-run is
    // zero-mutation and skips the prompt — the plan output needs an explicit
    // mode to display, and scripts that use --dry-run must not block on a
    // TTY prompt.
    let preserve_data = if args.dry_run {
        if !args.keep_data && !args.purge {
            return Err(CliError::uninstall(
                "dry-run requires --keep-data or --purge to pick a mode",
            ));
        }
        args.keep_data
    } else {
        let stdin = io::stdin();
        let is_tty = stdin.is_terminal();
        let mut reader = stdin.lock();
        let mut writer = io::stderr().lock();
        let Some(preserve_data) = resolve_mode(&args, is_tty, &mut reader, &mut writer)? else {
            return Ok("uninstall cancelled".to_string());
        };
        preserve_data
    };

    // R3-P0-04: `--dry-run` must be zero-mutation (UNS-001). The full
    // `acquire_update_lock` creates the data directory and lock file; use a
    // read-only `check_update_lock` instead. The non-dry-run path acquires
    // the real lock below, before any deletion.
    if args.dry_run {
        crate::lifecycle::update::check_update_lock().map_err(|e| {
            CliError::uninstall(format!("could not check update lock: {}", e.message))
        })?;
    }

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
                // R9-H12: skip symlink siblings. A symlink named
                // .math_talk_radar.rollback → /etc would otherwise be
                // canonicalized+deleted by delete_path. reject_symlink_in_components
                // in delete_path also guards this, but skipping here keeps
                // the dry-run plan honest (it won't list a symlink sibling
                // as a deletion target).
                if let Ok(meta) = std::fs::symlink_metadata(entry.path())
                    && meta.is_symlink()
                {
                    continue;
                }
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

    // H12: serialize against `update`. Without this lock, a concurrent
    // `update` could be mid-rename while `uninstall` deletes the binary and
    // rollback copy — update's post-replace self-test would then run against
    // a deleted path, or its `rename` would fail into a half-deleted tree.
    // The lock-failure error is remapped to an uninstall-fatal code (exit 11)
    // so the exit code matches the command the user ran, not the shared
    // lock's origin subsystem.
    let _lock = crate::lifecycle::update::acquire_update_lock().map_err(|e| {
        CliError::uninstall(format!("could not acquire update lock: {}", e.message))
    })?;

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
///
/// R9-B07: before canonicalizing, reject any symlink in the path's
/// components. Without this, if the app dir itself was a symlink to an
/// unprotected dir, canonicalize would resolve to the target and
/// remove_dir_all would delete it. The component walker catches symlinks
/// before resolution.
fn delete_path(path: &Path, is_binary: bool) -> Result<(), CliError> {
    paths::reject_symlink_in_components(path).map_err(CliError::uninstall)?;
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

/// §35.1 / §35.2: resolve the uninstall mode. Returns `Some(preserve_data)` to
/// proceed, or `None` if the user cancelled at the TTY prompt. Non-TTY without
/// `--yes` is refused (§35.2). `--yes` with an explicit `--keep-data`/`--purge`
/// skips the prompt (scriptable). TTY without an explicit mode offers the
/// three-way choice from §35.1.
fn resolve_mode<R: BufRead, W: Write>(
    args: &UninstallArgs,
    is_tty: bool,
    reader: &mut R,
    writer: &mut W,
) -> Result<Option<bool>, CliError> {
    match (args.keep_data, args.purge, args.yes) {
        (true, false, true) => return Ok(Some(true)),
        (false, true, true) => return Ok(Some(false)),
        (false, false, true) => {
            return Err(CliError::uninstall(
                "--yes requires --keep-data or --purge to pick a mode",
            ));
        }
        _ => {}
    }

    if !is_tty {
        return Err(CliError::uninstall(
            "noninteractive shell: pass --keep-data --yes or --purge --yes (see --help)",
        ));
    }

    let preserve_data = if args.keep_data {
        true
    } else if args.purge {
        false
    } else {
        writeln!(
            writer,
            "math_talk_radar uninstall\n\
             [1] remove program + config + cache, keep data (default)\n\
             [2] remove everything (including data)\n\
             [3] cancel"
        )
        .map_err(io_err)?;
        let choice = prompt_line(reader, writer, "select [1-3]: ")?;
        match choice.trim() {
            "" | "1" => true,
            "2" => false,
            "3" | "c" | "cancel" | "q" | "quit" => return Ok(None),
            other => {
                return Err(CliError::uninstall(format!(
                    "invalid selection {other:?}: expected 1, 2, or 3"
                )));
            }
        }
    };

    let label = if preserve_data { "keep-data" } else { "purge" };
    let confirm = prompt_line(reader, writer, &format!("confirm {label}? [y/N]: "))?;
    if !matches!(confirm.trim().to_lowercase().as_str(), "y" | "yes") {
        return Ok(None);
    }
    Ok(Some(preserve_data))
}

fn prompt_line<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
) -> Result<String, CliError> {
    writer.write_all(prompt.as_bytes()).map_err(io_err)?;
    writer.flush().map_err(io_err)?;
    let mut line = String::new();
    reader.read_line(&mut line).map_err(io_err)?;
    Ok(line)
}

fn io_err(e: io::Error) -> CliError {
    CliError::uninstall(format!("prompt I/O error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::UninstallArgs;

    fn args(keep_data: bool, purge: bool, yes: bool) -> UninstallArgs {
        UninstallArgs {
            dry_run: false,
            keep_data,
            purge,
            yes,
            force_unmanaged: false,
        }
    }

    fn run_resolve(a: &UninstallArgs, is_tty: bool, stdin: &str) -> Result<Option<bool>, CliError> {
        let mut reader = io::Cursor::new(stdin.as_bytes());
        let mut writer = Vec::<u8>::new();
        let result = resolve_mode(a, is_tty, &mut reader, &mut writer);
        let _ = String::from_utf8(writer).unwrap_or_default();
        result
    }

    #[test]
    fn yes_keep_data_skips_prompt() {
        assert_eq!(
            run_resolve(&args(true, false, true), false, "").unwrap(),
            Some(true)
        );
    }

    #[test]
    fn yes_purge_skips_prompt() {
        assert_eq!(
            run_resolve(&args(false, true, true), false, "").unwrap(),
            Some(false)
        );
    }

    #[test]
    fn yes_without_mode_is_error() {
        assert!(run_resolve(&args(false, false, true), false, "").is_err());
    }

    #[test]
    fn non_tty_without_yes_refused() {
        let err = run_resolve(&args(true, false, false), false, "").unwrap_err();
        assert!(
            err.message.contains("noninteractive"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn tty_default_choice_keeps_data() {
        let a = args(false, false, false);
        assert_eq!(run_resolve(&a, true, "\ny\n").unwrap(), Some(true));
    }

    #[test]
    fn tty_choice_2_purge() {
        let a = args(false, false, false);
        assert_eq!(run_resolve(&a, true, "2\ny\n").unwrap(), Some(false));
    }

    #[test]
    fn tty_choice_3_cancels() {
        let a = args(false, false, false);
        assert_eq!(run_resolve(&a, true, "3\n").unwrap(), None);
    }

    #[test]
    fn tty_confirm_no_cancels() {
        let a = args(false, false, false);
        assert_eq!(run_resolve(&a, true, "1\nn\n").unwrap(), None);
    }

    #[test]
    fn tty_explicit_keep_data_skips_menu_but_confirms() {
        let a = args(true, false, false);
        assert_eq!(run_resolve(&a, true, "y\n").unwrap(), Some(true));
    }

    #[test]
    fn tty_explicit_purge_skips_menu_but_confirms() {
        let a = args(false, true, false);
        assert_eq!(run_resolve(&a, true, "y\n").unwrap(), Some(false));
    }

    #[test]
    fn tty_invalid_choice_errors() {
        let a = args(false, false, false);
        assert!(run_resolve(&a, true, "9\n").is_err());
    }
}
