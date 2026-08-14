//! Known app-owned path resolution (§35, §36). All paths are canonicalized
//! before deletion to prevent symlink-traversal attacks. No path is ever
//! empty, `/`, or `$HOME`.
use std::path::{Path, PathBuf};

pub const APP_SLUG: &str = "math_talk_radar";

/// B3: the release API origin is a fixed constant per §34.3 ("HTTPS only; fixed
/// release repo"). The previous `MATH_TALK_RADAR_RELEASE_API` env var was
/// respected in production builds, allowing an attacker who could plant an env
/// var to redirect self-update to a malicious server. The override is now gated
/// on `debug_assertions` so only debug/test builds (which integration tests use
/// to point at a wiremock server) honor it; release binaries always use the
/// fixed GitHub origin.
pub const RELEASE_API_ENV: &str = "MATH_TALK_RADAR_RELEASE_API";
pub const DEFAULT_RELEASE_API: &str = "https://api.github.com/repos/Develata/math_talk_radar";

pub fn release_api() -> String {
    if cfg!(debug_assertions)
        && let Ok(api) = std::env::var(RELEASE_API_ENV)
    {
        return api;
    }
    DEFAULT_RELEASE_API.to_string()
}

/// The binary path to manage. Prefers the install manifest when present;
/// falls back to `current_exe`.
///
/// B2: the manifest's `binary_path` is only trusted if it looks like a real
/// app binary — the file name must contain `APP_SLUG`. A tampered manifest
/// could otherwise point at an arbitrary file (e.g. `~/.ssh/id_rsa`,
/// `/etc/passwd`) and `uninstall` would delete it. `safe_canonicalize` at
/// delete time blocks `/`, `$HOME`, and empty, but that is not enough: any
/// other path would pass. This filename check is defense-in-depth; it makes
/// the attack require not just write access to the manifest but also a target
/// filename containing `math_talk_radar`, which dramatically narrows the
/// blast radius.
pub fn binary_path(data_dir: &Path) -> Option<PathBuf> {
    if let Some(m) = crate::lifecycle::manifest::load(data_dir)
        && m.binary_path.exists()
        && is_plausible_app_binary(&m.binary_path)
    {
        return Some(m.binary_path);
    }
    std::env::current_exe().ok()
}

/// True if `path` looks like a `math_talk_radar` binary: the file name must
/// contain the app slug. This is a sanity check, not a security boundary —
/// `safe_canonicalize` at delete time provides the hard block on `/`, `$HOME`,
/// and empty.
fn is_plausible_app_binary(path: &Path) -> bool {
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

/// Temp directory for update downloads: sibling of the binary, prefixed
/// `.{binary_name}.` so stale files are identifiable (§34.3).
pub fn temp_dir_for_binary(binary: &Path) -> PathBuf {
    let parent = binary.parent().unwrap_or_else(|| Path::new("."));
    let stem = binary
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("math_talk_radar");
    let mut name = String::from(".");
    name.push_str(stem);
    name.push_str(".update.");
    name.push_str(&chrono::Utc::now().timestamp().to_string());
    parent.join(name)
}

/// Canonicalize and validate a path is safe to delete. Rejects empty, `/`,
/// and the user's home directory. Returns the canonical path on success.
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
        return PathBuf::from(xdg).join(APP_SLUG);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(default_sub).join(APP_SLUG);
    }
    PathBuf::from(default_sub).join(APP_SLUG)
}
