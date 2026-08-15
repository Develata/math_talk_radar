//! Known app-owned path resolution (§35, §36). All paths are canonicalized
//! before deletion to prevent symlink-traversal attacks. No path is ever
//! empty, `/`, or `$HOME`.
use std::path::{Path, PathBuf};

pub const APP_SLUG: &str = "math_talk_radar";

pub const DEFAULT_RELEASE_API: &str = "https://api.github.com/repos/Develata/math_talk_radar";

/// B3-1: the release API origin. Production always uses `DEFAULT_RELEASE_API`.
/// The `MATH_TALK_RADAR_RELEASE_API` env var is honored only when
/// `cfg!(debug_assertions)` is true (standard Rust compile-time gate for
/// debug/test builds). A distributor who sets `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true`
/// could technically leak this into a release build — but if they control the
/// build profile, they can modify source code directly, so this is a supply-chain
/// trust boundary, not a runtime attack surface. Defense-in-depth: the env var
/// URL is validated to be HTTPS or localhost, and `validate_download_url()`
/// independently locks download URLs to github.com regardless of where the API
/// metadata came from.
pub fn release_api() -> String {
    if cfg!(debug_assertions)
        && let Ok(api) = std::env::var("MATH_TALK_RADAR_RELEASE_API")
        && validate_api_origin(&api).is_ok()
    {
        return api;
    }
    DEFAULT_RELEASE_API.to_string()
}

/// Defense-in-depth: validate that an override API URL is HTTPS or points to
/// localhost (for wiremock tests). Prevents an env-var-based redirect to an
/// arbitrary HTTP server even if `debug_assertions` leaks.
fn validate_api_origin(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid API URL: {e}"))?;
    if parsed.scheme() == "https" {
        return Ok(());
    }
    if parsed.scheme() == "http"
        && matches!(parsed.host_str(), Some("127.0.0.1") | Some("localhost"))
    {
        return Ok(());
    }
    Err(format!(
        "API override must be HTTPS or localhost, got: {url}"
    ))
}

/// The binary path to manage. Prefers the install manifest when present;
/// falls back to `current_exe`.
///
/// B2-1: the manifest's `binary_path` is only trusted if it is a **regular
/// file** (not a directory, not a symlink) whose filename contains
/// `APP_SLUG`. `Path::exists()` follows symlinks and accepts directories,
/// so `symlink_metadata` is used to verify `is_file()` without following.
/// A tampered manifest pointing at a directory like
/// `~/math_talk_radar_backup` would otherwise cause `remove_dir_all` to
/// recursively delete it. Combined with `safe_canonicalize` at delete time
/// (which blocks protected system/user dirs), this makes the attack require
/// a regular file with `math_talk_radar` in its name outside protected
/// paths — a dramatically narrowed blast radius.
pub fn binary_path(data_dir: &Path) -> Option<PathBuf> {
    if let Some(m) = crate::lifecycle::manifest::load(data_dir)
        && is_plausible_app_binary(&m.binary_path)
    {
        return Some(m.binary_path);
    }
    std::env::current_exe().ok()
}

/// True if `path` looks like a `math_talk_radar` binary: must be a regular
/// file (not a directory, not a symlink) whose name contains `APP_SLUG`.
/// Uses `symlink_metadata` to avoid following symlinks (§35 "never follows
/// symlinks").
fn is_plausible_app_binary(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| name.contains(APP_SLUG))
        .unwrap_or(false)
}

/// `$XDG_CONFIG_HOME/math_talk_radar` or `~/.config/math_talk_radar`.
pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_CACHE_HOME/math_talk_radar` or `~/.cache/math_talk_radar`.
pub fn cache_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_DATA_HOME/math_talk_radar` or `~/.local/share/math_talk_radar`.
pub fn data_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

/// Temp directory for update downloads: sibling of the binary, with a
/// random suffix so the staging path is unpredictable (B05). A predictable
/// path like `.{stem}.update.{unix_seconds}` lets a local attacker pre-create
/// a symlink at that exact location and have the download overwrite the
/// symlink target. The suffix mixes nanosecond time + PID for uniqueness
/// across concurrent invocations.
pub fn temp_dir_for_binary(binary: &Path) -> PathBuf {
    let parent = binary.parent().unwrap_or_else(|| Path::new("."));
    let stem = binary
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("math_talk_radar");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let pid = std::process::id();
    let name = format!(".{stem}.update.{pid}.{nanos}");
    parent.join(name)
}

/// Canonicalize and validate a path is safe to delete. Rejects empty, `/`,
/// the user's home directory, and paths inside protected system/user
/// directories (§35 "deletes only known app-owned paths"). The protected
/// list covers credentials, system config, and kernel virtual filesystems
/// that should never be touched by an uninstaller regardless of manifest
/// content.
pub fn safe_canonicalize(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize {}: {e}", path.display()))?;
    if canonical.as_os_str().is_empty() {
        return Err("refusing to delete empty path".into());
    }
    if canonical == Path::new("/") {
        return Err("refusing to delete /".into());
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if canonical == home {
            return Err("refusing to delete $HOME".into());
        }
    }
    let path_str = canonical.to_string_lossy();
    const PROTECTED: &[&str] = &[
        "/etc", "/proc", "/sys", "/dev", "/boot", "/bin", "/sbin", "/lib", "/lib64", "/usr",
        "/var/log",
    ];
    for p in PROTECTED {
        if path_str.starts_with(p) {
            return Err(format!(
                "refusing to delete protected system path under {p}"
            ));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        let home_protected = [".ssh", ".gnupg", ".config/systemd", ".local/share/systemd"];
        for sub in home_protected {
            let protected = home.join(sub);
            if path_str.starts_with(protected.to_string_lossy().as_ref()) {
                return Err(format!(
                    "refusing to delete protected user path under {sub}"
                ));
            }
        }
    }
    Ok(canonical)
}

/// True if `binary` looks like a `cargo run` / `target/debug` development
/// binary (§36). Used by uninstall to protect dev binaries.
pub fn is_unmanaged_binary(binary: &Path) -> bool {
    let s = binary.to_string_lossy();
    s.contains("/target/debug/") || s.contains("/target/release/")
}

fn xdg_dir(env_var: &str, default_sub: &str) -> PathBuf {
    if let Some(xdg) = std::env::var_os(env_var) {
        let p = PathBuf::from(&xdg);
        // B08: reject empty or relative XDG values. An empty value yields a
        // relative `math_talk_radar` path anchored at the CWD — uninstall's
        // safe_canonicalize would resolve it to the CWD and (if the CWD is
        // not protected) delete files there. A relative value like `./foo`
        // has the same hazard. Fall through to the default instead.
        if !xdg.is_empty() && p.is_absolute() {
            return p.join(APP_SLUG);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(default_sub).join(APP_SLUG);
    }
    PathBuf::from(default_sub).join(APP_SLUG)
}

#[cfg(test)]
mod tests {
    use super::*;

    // R9-B05: two staging paths for the same binary must differ — the suffix
    // is random (PID + nanos), not a predictable timestamp.
    #[test]
    fn temp_dir_for_binary_is_unique_per_call() {
        let bin = Path::new("/usr/local/bin/math_talk_radar");
        let a = temp_dir_for_binary(bin);
        let b = temp_dir_for_binary(bin);
        assert_ne!(a, b, "staging paths must be unpredictable per call");
    }

    // R9-B05: the staging path retains the binary stem prefix so stale files
    // are identifiable.
    #[test]
    fn temp_dir_for_binary_preserves_stem_prefix() {
        let bin = Path::new("/opt/app/math_talk_radar");
        let p = temp_dir_for_binary(bin);
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with(".math_talk_radar.update."),
            "staging name must start with .{{stem}}.update., got {name}"
        );
    }

    // R9-B05: the staging path is a sibling of the binary, not in /tmp.
    #[test]
    fn temp_dir_for_binary_is_sibling_of_binary() {
        let bin = Path::new("/usr/local/bin/math_talk_radar");
        let p = temp_dir_for_binary(bin);
        assert_eq!(p.parent(), bin.parent());
    }
}
