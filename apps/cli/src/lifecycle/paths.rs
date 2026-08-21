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

/// R9-B07: reject any path that contains a symlink in its components. The
/// previous `safe_canonicalize` validated the canonical target but then
/// deleted it — if the app dir was a symlink to an unprotected dir, the
/// canonical target was deleted. This walker checks every component of the
/// *un-resolved* path using `symlink_metadata` (which does not follow the
/// final symlink), so a symlink anywhere in the path is detected before
/// canonicalization. Only absolute paths are supported; relative paths are
/// rejected (deletion should only ever target resolved absolute paths).
///
/// The walk stops at `/` (or the first component that does not exist,
/// which is safe — a non-existent path cannot be a symlink). Each existing
/// component is checked with `symlink_metadata`; if `is_symlink()` is true,
/// the path is rejected.
pub fn reject_symlink_in_components(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "refusing to operate on relative path (require absolute): {}",
            path.display()
        ));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::RootDir => {
                current.push("/");
                continue;
            }
            Component::Normal(part) => {
                current.push(part);
            }
            Component::CurDir | Component::ParentDir => {
                return Err(format!(
                    "refusing to operate on path with . or .. components: {}",
                    path.display()
                ));
            }
            Component::Prefix(_) => {
                return Err(format!(
                    "refusing to operate on Windows-style path: {}",
                    path.display()
                ));
            }
        }
        if let Ok(meta) = std::fs::symlink_metadata(&current)
            && meta.is_symlink()
        {
            return Err(format!(
                "refusing to operate on path with symlink component: {} -> (symlink at {})",
                path.display(),
                current.display()
            ));
        }
    }
    Ok(())
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

/// True if `binary` looks like a `cargo run` / `target/.../debug` or
/// `target/.../release` development binary (§36). Used by uninstall to
/// protect dev binaries.
///
/// Cargo's target dir can be customized via `CARGO_TARGET_DIR`,
/// `build.target-dir`, or tools like `cargo-llvm-cov` (which uses
/// `target/llvm-cov-target/`). Instead of hardcoding `/target/debug/`
/// and `/target/release/`, check if the path contains `/target/` and a
/// `/debug/` or `/release/` profile component.
pub fn is_unmanaged_binary(binary: &Path) -> bool {
    let s = binary.to_string_lossy();
    s.contains("/target/") && (s.contains("/debug/") || s.contains("/release/"))
}

fn xdg_dir(env_var: &str, default_sub: &str) -> PathBuf {
    resolve_xdg_dir(
        std::env::var_os(env_var).as_deref(),
        std::env::var_os("HOME").as_deref(),
        default_sub,
    )
}

/// Pure core of `xdg_dir` for testability. B08: validates both the XDG
/// override and the HOME fallback for nonempty + absolute. A relative or
/// empty value resolves against the CWD; `safe_canonicalize` would then
/// resolve it to the CWD and (if the CWD is not protected) `remove_dir_all`
/// would delete files in the working directory. An invalid HOME must fall
/// through to the relative default rather than producing a CWD-anchored path.
fn resolve_xdg_dir(
    xdg_var: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    default_sub: &str,
) -> PathBuf {
    if let Some(xdg) = xdg_var
        && !xdg.is_empty()
    {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            return p.join(APP_SLUG);
        }
    }
    if let Some(home) = home
        && !home.is_empty()
    {
        let p = PathBuf::from(home);
        if p.is_absolute() {
            return p.join(default_sub).join(APP_SLUG);
        }
    }
    PathBuf::from(default_sub).join(APP_SLUG)
}

/// B08: detect whether any two of the config/cache/data dirs canonicalize to
/// the same path. Such overlap (from a misconfigured XDG setup) is dangerous
/// for uninstall: `--keep-data` deletes config+cache but preserves data, so
/// if config == data, data would be deleted despite --keep-data. Refuse
/// rather than risk data loss. Dirs that do not exist (cannot canonicalize)
/// are skipped — they would be skipped at delete time too.
pub fn detect_dir_overlap(config: &Path, cache: &Path, data: &Path) -> Result<(), String> {
    let mut canonicals: Vec<(&str, PathBuf)> = Vec::new();
    for (name, path) in [("config", config), ("cache", cache), ("data", data)] {
        if let Ok(c) = path.canonicalize() {
            canonicals.push((name, c));
        }
    }
    for i in 0..canonicals.len() {
        for j in (i + 1)..canonicals.len() {
            if canonicals[i].1 == canonicals[j].1 {
                return Err(format!(
                    "directory overlap detected: {} and {} resolve to the same path ({}); \
                     refusing to proceed to avoid data loss",
                    canonicals[i].0,
                    canonicals[j].0,
                    canonicals[i].1.display()
                ));
            }
        }
    }
    Ok(())
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

    // R9-B08: a valid absolute HOME produces the default XDG path under HOME.
    #[test]
    fn resolve_xdg_dir_uses_absolute_home() {
        let home = std::ffi::OsStr::new("/home/deve");
        let p = resolve_xdg_dir(None, Some(home), ".config");
        assert_eq!(p, PathBuf::from("/home/deve/.config/math_talk_radar"));
    }

    // R9-B08: an absolute XDG override wins over HOME.
    #[test]
    fn resolve_xdg_dir_xdg_override_wins() {
        let xdg = std::ffi::OsStr::new("/custom/cfg");
        let home = std::ffi::OsStr::new("/home/deve");
        let p = resolve_xdg_dir(Some(xdg), Some(home), ".config");
        assert_eq!(p, PathBuf::from("/custom/cfg/math_talk_radar"));
    }

    // R9-B08: an empty HOME must NOT produce a CWD-anchored path; fall through
    // to the relative default instead.
    #[test]
    fn resolve_xdg_dir_empty_home_falls_through() {
        let empty = std::ffi::OsStr::new("");
        let p = resolve_xdg_dir(None, Some(empty), ".config");
        assert_eq!(p, PathBuf::from(".config/math_talk_radar"));
    }

    // R9-B08: a relative HOME must NOT be used; fall through to the relative
    // default instead (a relative HOME would anchor at CWD).
    #[test]
    fn resolve_xdg_dir_relative_home_falls_through() {
        let rel = std::ffi::OsStr::new("relative/home");
        let p = resolve_xdg_dir(None, Some(rel), ".local/share");
        assert_eq!(p, PathBuf::from(".local/share/math_talk_radar"));
    }

    // R9-B08: a relative XDG override must NOT be used; fall through to HOME.
    #[test]
    fn resolve_xdg_dir_relative_xdg_falls_to_home() {
        let rel_xdg = std::ffi::OsStr::new("relative/cfg");
        let home = std::ffi::OsStr::new("/home/deve");
        let p = resolve_xdg_dir(Some(rel_xdg), Some(home), ".config");
        assert_eq!(p, PathBuf::from("/home/deve/.config/math_talk_radar"));
    }

    // R9-B08: an empty XDG override must NOT be used; fall through to HOME.
    #[test]
    fn resolve_xdg_dir_empty_xdg_falls_to_home() {
        let empty = std::ffi::OsStr::new("");
        let home = std::ffi::OsStr::new("/home/deve");
        let p = resolve_xdg_dir(Some(empty), Some(home), ".cache");
        assert_eq!(p, PathBuf::from("/home/deve/.cache/math_talk_radar"));
    }

    // R9-B08: no XDG and no HOME → relative default (last resort).
    #[test]
    fn resolve_xdg_dir_no_env_falls_to_relative_default() {
        let p = resolve_xdg_dir(None, None, ".config");
        assert_eq!(p, PathBuf::from(".config/math_talk_radar"));
    }

    // R9-B08: detect_dir_overlap must reject when config and data canonicalize
    // to the same path (e.g. misconfigured XDG_CONFIG_HOME == XDG_DATA_HOME).
    #[test]
    fn detect_dir_overlap_rejects_config_data_collision() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let same = tmp.path();
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).expect("mkdir other");
        let err = detect_dir_overlap(same, &other, same);
        assert!(err.is_err(), "config==data overlap must be rejected");
        let msg = err.unwrap_err();
        assert!(msg.contains("config") && msg.contains("data"), "msg: {msg}");
    }

    // R9-B08: detect_dir_overlap must reject when cache and data collide.
    #[test]
    fn detect_dir_overlap_rejects_cache_data_collision() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let same = tmp.path();
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).expect("mkdir other");
        let err = detect_dir_overlap(&other, same, same);
        assert!(err.is_err(), "cache==data overlap must be rejected");
        assert!(err.unwrap_err().contains("cache"));
    }

    // R9-B08: detect_dir_overlap must reject when config and cache collide.
    #[test]
    fn detect_dir_overlap_rejects_config_cache_collision() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let same = tmp.path();
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).expect("mkdir other");
        let err = detect_dir_overlap(same, same, &other);
        assert!(err.is_err(), "config==cache overlap must be rejected");
        assert!(err.unwrap_err().contains("config"));
    }

    // R9-B08: detect_dir_overlap passes when all three dirs are distinct.
    #[test]
    fn detect_dir_overlap_ok_when_distinct() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        let cfg = root.join("cfg");
        let cache = root.join("cache");
        let data = root.join("data");
        for d in [&cfg, &cache, &data] {
            std::fs::create_dir_all(d).expect("mkdir");
        }
        assert!(detect_dir_overlap(&cfg, &cache, &data).is_ok());
    }

    // R9-B08: detect_dir_overlap passes when dirs don't exist yet (skip those
    // that can't canonicalize — they'd be skipped at delete time too).
    #[test]
    fn detect_dir_overlap_skips_nonexistent() {
        assert!(
            detect_dir_overlap(
                Path::new("/nonexistent/cfg/zzz"),
                Path::new("/nonexistent/cache/zzz"),
                Path::new("/nonexistent/data/zzz"),
            )
            .is_ok(),
            "nonexistent dirs are skipped, not treated as overlapping"
        );
    }

    // R9-B07: a relative path must be rejected outright.
    #[test]
    fn reject_symlink_in_components_rejects_relative() {
        let err = reject_symlink_in_components(Path::new("relative/path/file"));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("relative"));
    }

    // R9-B07: a path with `..` components must be rejected (would escape the
    // caller's intended directory).
    #[test]
    fn reject_symlink_in_components_rejects_parent_dir() {
        let err = reject_symlink_in_components(Path::new("/a/../b/c"));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains(".."));
    }

    // R9-B07: a leaf symlink must be rejected. The caller must operate on the
    // real file, not whatever the symlink points at.
    #[cfg(unix)]
    #[test]
    fn reject_symlink_in_components_rejects_leaf_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().expect("temp dir");
        let target = tmp.path().join("target");
        std::fs::write(&target, b"body").expect("write target");
        let link = tmp.path().join("link");
        symlink(&target, &link).expect("symlink");
        let err = reject_symlink_in_components(&link);
        assert!(err.is_err(), "leaf symlink must be rejected");
        let msg = err.unwrap_err();
        assert!(msg.contains("symlink"), "msg: {msg}");
    }

    // R9-B07: a mid-path symlink (a symlink in a non-leaf component) must be
    // rejected. Without this check, an attacker could plant a symlink on a
    // parent directory component to redirect the resolved path.
    #[cfg(unix)]
    #[test]
    fn reject_symlink_in_components_rejects_midpath_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().expect("temp dir");
        let real_dir = tmp.path().join("real");
        std::fs::create_dir_all(&real_dir).expect("mkdir real");
        let link_dir = tmp.path().join("linkdir");
        symlink(&real_dir, &link_dir).expect("symlink midpath");
        let target = link_dir.join("file");
        let err = reject_symlink_in_components(&target);
        assert!(err.is_err(), "midpath symlink must be rejected");
        assert!(err.unwrap_err().contains("symlink"));
    }

    // R9-B07: a clean path with no symlinks anywhere in its components must
    // pass.
    #[cfg(unix)]
    #[test]
    fn reject_symlink_in_components_accepts_clean_path() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let real = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&real).expect("mkdir");
        assert!(reject_symlink_in_components(&real).is_ok());
    }

    // R9-B07: a path that doesn't exist yet (some components missing) must
    // pass — missing components cannot be symlinks.
    #[test]
    fn reject_symlink_in_components_accepts_nonexistent_leaf() {
        let p = Path::new("/tmp/definitely/not/here/zzz_not_existing_12345");
        assert!(reject_symlink_in_components(p).is_ok());
    }

    #[test]
    fn is_unmanaged_binary_recognizes_standard_target_dirs() {
        assert!(is_unmanaged_binary(Path::new(
            "/home/u/proj/target/debug/math_talk_radar"
        )));
        assert!(is_unmanaged_binary(Path::new(
            "/home/u/proj/target/release/math_talk_radar"
        )));
    }

    #[test]
    fn is_unmanaged_binary_recognizes_custom_target_dirs() {
        assert!(is_unmanaged_binary(Path::new(
            "/home/runner/work/proj/proj/target/llvm-cov-target/debug/math_talk_radar"
        )));
    }

    #[test]
    fn is_unmanaged_binary_rejects_managed_paths() {
        assert!(!is_unmanaged_binary(Path::new(
            "/usr/local/bin/math_talk_radar"
        )));
        assert!(!is_unmanaged_binary(Path::new(
            "/home/user/.local/bin/math_talk_radar"
        )));
    }
}
