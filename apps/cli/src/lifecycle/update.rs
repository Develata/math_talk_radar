//! Self-update (§34). Fetch latest release, SemVer compare, download, verify
//! SHA-256, self-test, rollback, atomic replace.
use std::path::{Path, PathBuf};

use futures::StreamExt;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::lifecycle::paths;
use crate::runtime::CliError;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE_USER_AGENT: &str = concat!(
    "math_talk_radar/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Develata/math_talk_radar)"
);

/// R9-H11: bounds for update downloads. Without these a malicious or
/// misbehaving CDN could stream indefinitely, hanging the updater or
/// filling the disk. The binary cap leaves headroom over the largest real
/// musl release (~8 MiB); the checksum is a single 64-hex-char line. The
/// overall deadline bounds connect+download+read so a slow-drip server
/// cannot hold the lock forever.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const MAX_BINARY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 1024;

/// H7: independent update lock (§34.3). Without this, two concurrent
/// `math_talk_radar update` invocations would race on the same binary:
/// both download, both verify, both try atomic-replace — one wins, the
/// other's rollback/temp files are left orphaned, and the self-test of the
/// loser runs against a binary the winner already replaced. The lock is a
/// lockfile at `data_dir/update.lock` created with `O_CREAT|O_EXCL` (via
/// `OpenOptions::create_new`), which is atomic across processes. A crash
/// during update leaves a stale lockfile; the next `update` detects it by
/// checking whether the recorded PID is still alive, and steals the lock if
/// not. `#![forbid(unsafe_code)]` rules out `flock(2)`, so this userspace
/// approach is used instead.
///
/// H7-1/H7-3: The lock file stores `PID:STARTTIME:TOKEN` where STARTTIME is
/// the process start time from `/proc/<pid>/stat` (protects against PID
/// reuse) and TOKEN is a per-session random value (prevents a later guard's
/// Drop from removing a different process's lock). Stale recovery uses
/// atomic `rename` to a tombstone — only one contender wins, eliminating the
/// TOCTOU race where two processes both `remove_file` + `create_new`.
///
/// H12: the same lock serializes `uninstall` vs `update`. Without it, a
/// concurrent `uninstall` could delete the binary and rollback copy while
/// `update` is mid-replace, leaving update's `rename` or post-replace
/// self-test running against a deleted path. `uninstall` acquires this lock
/// before resolving or deleting any path.
pub(crate) struct UpdateGuard {
    path: PathBuf,
    token: u64,
}

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        // H7-1: only remove if our token still matches — another process may
        // have stolen the lock after we crashed.
        if let Ok(content) = std::fs::read_to_string(&self.path)
            && content.trim().ends_with(&self.token.to_string())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// H7-2/H7-3: write PID, process start time, and a random ownership token.
/// Returns the token so the guard can verify ownership on cleanup.
fn write_lock_content(file: &mut std::fs::File) -> Result<u64, CliError> {
    use std::io::Write;
    let pid = std::process::id();
    if pid == 0 {
        return Err(CliError::update("PID 0 is not a valid updater identity"));
    }
    let starttime = process_starttime(pid).unwrap_or(0);
    // Simple entropy: nanosecond timestamp. Not cryptographically random, but
    // sufficient to distinguish two concurrent update invocations.
    let token = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let _ = writeln!(file, "{pid}:{starttime}:{token}");
    Ok(token)
}

pub(crate) fn acquire_update_lock() -> Result<UpdateGuard, CliError> {
    let lock_path = paths::data_dir().join("update.lock");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::update(format!("create data dir for lock: {e}")))?;
    }
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(mut f) => {
            let token = write_lock_content(&mut f)?;
            Ok(UpdateGuard {
                path: lock_path,
                token,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // H7-1: Stale-lock recovery via atomic rename. Two processes may
            // both inspect the stale lock, but only one can successfully
            // rename it — the loser's rename fails and it retries the
            // create_new path instead of clobbering the winner's lock.
            let steal = std::fs::read_to_string(&lock_path)
                .ok()
                .and_then(|c| parse_lock_pid(&c))
                .map(|(pid, starttime)| {
                    if pid == 0 {
                        return true;
                    }
                    !is_process_alive_with_starttime(pid, starttime)
                })
                .unwrap_or(true);
            if !steal {
                return Err(CliError::update(
                    "another update is in progress (update.lock is held)",
                ));
            }
            // Atomic rename to tombstone — only one contender wins.
            let tombstone = lock_path.with_extension("lock.stale");
            if std::fs::rename(&lock_path, &tombstone).is_err() {
                // Someone else already stole it; retry create_new.
                return acquire_update_lock();
            }
            let _ = std::fs::remove_file(&tombstone);
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&lock_path)
            {
                Ok(mut f) => {
                    let token = write_lock_content(&mut f)?;
                    Ok(UpdateGuard {
                        path: lock_path,
                        token,
                    })
                }
                Err(e2) => Err(CliError::update(format!(
                    "acquire update lock after steal: {e2}"
                ))),
            }
        }
        Err(e) => Err(CliError::update(format!("create update lock: {e}"))),
    }
}

/// Parse `PID:STARTTIME:TOKEN` from a lock file, returning (PID, STARTTIME).
fn parse_lock_pid(content: &str) -> Option<(u32, u64)> {
    let parts: Vec<&str> = content.trim().split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let pid = parts[0].parse::<u32>().ok()?;
    let starttime = parts[1].parse::<u64>().ok()?;
    Some((pid, starttime))
}

/// H7-2: Read process start time from `/proc/<pid>/stat` (field 22). Pure
/// filesystem read — no PATH search, no external command. Returns 0 if
/// `/proc` is unavailable (non-Linux).
#[cfg(target_os = "linux")]
fn process_starttime(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Field 22 is the process start time in clock ticks. Fields are
    // space-separated, but the comm field (field 2) may contain spaces
    // enclosed in parentheses. Find the last ')' and parse after it.
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // After ')', field 3 starts at index 0. Field 22 (starttime) is at
    // index 19 (22 - 3 = 19).
    fields.get(19).and_then(|s| s.parse::<u64>().ok())
}

#[cfg(not(target_os = "linux"))]
fn process_starttime(_pid: u32) -> Option<u64> {
    None
}

/// H7-2/H7-3: Check liveness using `/proc/<pid>` on Linux (no PATH search,
/// no external command). If `starttime` is nonzero, also verify the current
/// process start time matches — this detects PID reuse by a different
/// process after the original updater crashed.
#[cfg(target_os = "linux")]
fn is_process_alive_with_starttime(pid: u32, starttime: u64) -> bool {
    if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
        return false;
    }
    if starttime == 0 {
        return true;
    }
    match process_starttime(pid) {
        Some(current_starttime) => current_starttime == starttime,
        None => true, // Can't verify; assume alive to avoid stealing a live lock.
    }
}

/// H7-2: Non-Linux Unix fallback. Use absolute `/bin/kill -0` — no PATH
/// search. PID reuse detection is unavailable without `/proc`.
#[cfg(all(unix, not(target_os = "linux")))]
fn is_process_alive_with_starttime(pid: u32, _starttime: u64) -> bool {
    if pid == 0 {
        return false;
    }
    std::process::Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// H7-3: Non-Unix platforms cannot verify liveness. Fail safe: assume alive
/// so we don't steal a lock held by a process we can't see.
#[cfg(not(unix))]
fn is_process_alive_with_starttime(_pid: u32, _starttime: u64) -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

/// B3-residual: validate that a download URL from the GitHub release API
/// points to the expected origin. In release builds: HTTPS only, host must
/// be `github.com` or `objects.githubusercontent.com` (GitHub's release
/// asset CDN). In debug builds: allow `http://` + `127.0.0.1`/`localhost`
/// so integration tests can point at a local wiremock server.
fn validate_download_url(url: &str) -> Result<(), CliError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| CliError::update(format!("invalid download URL '{url}': {e}")))?;
    if cfg!(debug_assertions)
        && let Some(host) = parsed.host_str()
        && (parsed.scheme() == "http" || parsed.scheme() == "https")
        && (host == "127.0.0.1" || host == "localhost")
    {
        return Ok(());
    }
    if parsed.scheme() != "https" {
        return Err(CliError::update(format!(
            "download URL must be HTTPS, got '{}': {url}",
            parsed.scheme()
        )));
    }
    let host = parsed.host_str().unwrap_or("");
    if host != "github.com"
        && host != "objects.githubusercontent.com"
        && host != "codeload.github.com"
    {
        return Err(CliError::update(format!(
            "download URL host must be github.com or objects.githubusercontent.com, got '{host}'"
        )));
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client, CliError> {
    reqwest::Client::builder()
        .user_agent(RELEASE_USER_AGENT)
        .timeout(DOWNLOAD_TIMEOUT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| CliError::update(format!("http client build failed: {e}")))
}

async fn fetch_latest_release() -> Result<Release, CliError> {
    let api = paths::release_api();
    let url = format!("{api}/releases/latest");
    let client = http_client()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| CliError::update(format!("release fetch failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(CliError::update(format!(
            "release API returned {}",
            resp.status()
        )));
    }
    resp.json::<Release>()
        .await
        .map_err(|e| CliError::update(format!("release JSON parse failed: {e}")))
}

fn parse_tag(tag: &str) -> Result<Version, CliError> {
    let cleaned = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(cleaned)
        .map_err(|e| CliError::update(format!("invalid release tag '{tag}': {e}")))
}

/// `update --check`: fetch latest release metadata, compare versions, write
/// nothing (UPD-001). Returns a human-readable status message.
pub async fn check() -> Result<String, CliError> {
    let release = fetch_latest_release().await?;
    let latest = parse_tag(&release.tag_name)?;
    let current = Version::parse(CURRENT_VERSION).expect("CARGO_PKG_VERSION is valid semver");
    if latest > current {
        Ok(format!(
            "update available: {} (current {})",
            release.tag_name, CURRENT_VERSION
        ))
    } else {
        Ok(format!(
            "up to date: {} (latest {})",
            CURRENT_VERSION, release.tag_name
        ))
    }
}

fn find_assets(release: &Release) -> Result<(&ReleaseAsset, &ReleaseAsset), CliError> {
    const BINARY_NAME: &str = "math_talk_radar-x86_64-unknown-linux-musl";
    const CHECKSUM_NAME: &str = "math_talk_radar-x86_64-unknown-linux-musl.sha256";

    let binary = release
        .assets
        .iter()
        .find(|a| a.name == BINARY_NAME)
        .ok_or_else(|| CliError::update(format!("release missing asset '{BINARY_NAME}'")))?;
    let checksum = release
        .assets
        .iter()
        .find(|a| a.name == CHECKSUM_NAME)
        .ok_or_else(|| CliError::update(format!("release missing asset '{CHECKSUM_NAME}'")))?;
    Ok((binary, checksum))
}

/// R9-H11: download a bounded body into memory. `max_bytes` caps the total
/// size so a malicious CDN cannot exhaust memory. Used for the checksum
/// file (small). The binary uses the streaming path instead.
async fn download_bytes(url: &str, max_bytes: u64) -> Result<Vec<u8>, CliError> {
    let client = http_client()?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| CliError::update(format!("download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(CliError::update(format!(
            "download returned {}",
            resp.status()
        )));
    }
    if let Some(len) = resp.content_length()
        && len > max_bytes
    {
        return Err(CliError::update(format!(
            "download size {len} exceeds limit {max_bytes}"
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| CliError::update(format!("download body read failed: {e}")))?;
    if bytes.len() as u64 > max_bytes {
        return Err(CliError::update(format!(
            "download body {} bytes exceeds limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(bytes.to_vec())
}

/// Stream the download body to `dest` while hashing incrementally, returning
/// the SHA-256 hex digest. Bounds memory to the chunk size instead of
/// buffering the entire binary in RAM.
async fn download_to_file_with_hash(url: &str, dest: &Path) -> Result<String, CliError> {
    let client = http_client()?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| CliError::update(format!("download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(CliError::update(format!(
            "download returned {}",
            resp.status()
        )));
    }
    // R9-H11: reject oversized downloads early when the CDN declares a
    // Content-Length, then enforce it again in the streaming loop in case
    // the declared length was absent or a lie.
    if let Some(len) = resp.content_length()
        && len > MAX_BINARY_BYTES
    {
        return Err(CliError::update(format!(
            "download size {len} exceeds binary limit {MAX_BINARY_BYTES}"
        )));
    }
    // B05: create_new refuses to open an existing file or follow a symlink
    // at dest. A predictable staging path let a local attacker pre-create a
    // symlink there; File::create would follow it and overwrite the target.
    // create_new + the random suffix from temp_dir_for_binary close this.
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(dest)
        .map_err(|e| CliError::update(format!("create temp file failed: {e}")))?;
    // CLI-11: a mid-stream download or write error previously leaked the temp
    // file (only the checksum/self-test/rename paths cleaned up). Centralize
    // cleanup here so every error path inside the streaming loop removes it.
    let stream_result: Result<String, CliError> = async {
        let mut hasher = Sha256::new();
        let mut stream = resp.bytes_stream();
        let mut total: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| CliError::update(format!("download stream error: {e}")))?;
            total = total
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| CliError::update("download size overflowed u64"))?;
            if total > MAX_BINARY_BYTES {
                return Err(CliError::update(format!(
                    "download body exceeded binary limit {MAX_BINARY_BYTES} bytes"
                )));
            }
            hasher.update(&chunk);
            std::io::Write::write_all(&mut file, &chunk)
                .map_err(|e| CliError::update(format!("write temp file failed: {e}")))?;
        }
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
        }
        Ok(hex)
    }
    .await;
    if stream_result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    stream_result
}

/// Parse a `.sha256` file: first hex token is the digest, rest is filename.
fn parse_checksum_file(contents: &str) -> Result<String, CliError> {
    let first_token = contents
        .split_whitespace()
        .next()
        .ok_or_else(|| CliError::update("empty checksum file"))?;
    if !first_token.chars().all(|c| c.is_ascii_hexdigit()) || first_token.len() != 64 {
        return Err(CliError::update(format!(
            "invalid checksum digest: '{first_token}'"
        )));
    }
    Ok(first_token.to_lowercase())
}

/// Run `binary --version` and check exit 0 (self-test, §34.2).
fn self_test(binary: &Path) -> Result<(), CliError> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| CliError::update(format!("self-test spawn failed: {e}")))?;
    if !output.status.success() {
        return Err(CliError::update(format!(
            "self-test failed: exit {:?}",
            output.status.code()
        )));
    }
    Ok(())
}

/// `update`: full algorithm (§34.2). Refuse unmanaged binary unless
/// `force_unmanaged`. Download -> verify SHA-256 -> fsync -> self-test ->
/// rollback copy -> atomic replace -> self-test -> cleanup. Any failure leaves
/// the current binary usable.
pub async fn run(force_unmanaged: bool) -> Result<String, CliError> {
    // H7: acquire the update lock before any I/O. Two concurrent `update`
    // invocations would race on download/replace/rollback; the lock serializes
    // them. `--check` does not need the lock (read-only).
    let _lock = acquire_update_lock()?;
    let data_dir = paths::data_dir();
    let current_binary = paths::binary_path(&data_dir)
        .ok_or_else(|| CliError::update("cannot resolve current binary path"))?;
    // CLI-26: align with uninstall's manifest-aware dev-binary guard. A
    // manifest that manages a target/ path (e.g. from a prior --force-unmanaged
    // update) should be trusted, not refused — matching uninstall's behavior.
    let manifest = crate::lifecycle::manifest::load(&data_dir);
    let managed_by_manifest = manifest
        .as_ref()
        .map(|m| m.binary_path == current_binary)
        .unwrap_or(false);
    if !managed_by_manifest && !force_unmanaged && paths::is_unmanaged_binary(&current_binary) {
        return Err(CliError::update(format!(
            "refusing to update unmanaged binary: {} (use --force-unmanaged to override)",
            current_binary.display()
        )));
    }

    let release = fetch_latest_release().await?;
    let latest = parse_tag(&release.tag_name)?;
    let current = Version::parse(CURRENT_VERSION).expect("CARGO_PKG_VERSION is valid semver");
    if latest <= current {
        return Ok(format!(
            "up to date: {} (latest {})",
            CURRENT_VERSION, release.tag_name
        ));
    }

    let (binary_asset, checksum_asset) = find_assets(&release)?;
    validate_download_url(&checksum_asset.browser_download_url)?;
    validate_download_url(&binary_asset.browser_download_url)?;
    let checksum_bytes =
        download_bytes(&checksum_asset.browser_download_url, MAX_CHECKSUM_BYTES).await?;
    let expected_hash = parse_checksum_file(&String::from_utf8_lossy(&checksum_bytes))?;

    let temp_path = paths::temp_dir_for_binary(&current_binary);
    let actual_hash =
        download_to_file_with_hash(&binary_asset.browser_download_url, &temp_path).await?;
    if actual_hash != expected_hash {
        let _ = std::fs::remove_file(&temp_path);
        return Err(CliError::update(format!(
            "checksum mismatch: expected {expected_hash}, got {actual_hash}"
        )));
    }

    set_executable(&temp_path)?;
    fsync_file(&temp_path)?;

    // Self-test the candidate before touching the working binary.
    if let Err(e) = self_test(&temp_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }

    // Rollback copy alongside the binary.
    let rollback_path = rollback_path(&current_binary);
    std::fs::copy(&current_binary, &rollback_path)
        .map_err(|e| CliError::update(format!("create rollback failed: {e}")))?;
    fsync_file(&rollback_path)?;

    // Atomic replace. On Unix, rename over an existing file is atomic.
    if let Err(e) = std::fs::rename(&temp_path, &current_binary) {
        let _ = std::fs::remove_file(&temp_path);
        let _ = std::fs::remove_file(&rollback_path);
        return Err(CliError::update(format!("atomic replace failed: {e}")));
    }
    fsync_parent_dir(&current_binary)?;

    // Self-test the replaced binary; restore rollback on failure. The error
    // message must tell the user whether rollback succeeded and where the
    // rollback copy lives, so a double failure still leaves a recovery path.
    if let Err(e) = self_test(&current_binary) {
        let msg = match std::fs::rename(&rollback_path, &current_binary) {
            Ok(()) => format!(
                "{}; rollback restored to {}",
                e.message,
                current_binary.display()
            ),
            Err(re) => format!(
                "{}; rollback restore FAILED: {re}; \
                 recover manually from {} if still present",
                e.message,
                rollback_path.display()
            ),
        };
        return Err(CliError::update(msg));
    }

    let _ = std::fs::remove_file(&rollback_path);

    // Update manifest. The binary is already replaced and self-tested, so a
    // manifest write failure is NOT fatal — surface it as a warning in the
    // success message rather than turning a successful update into an error.
    let manifest = crate::lifecycle::manifest::InstallManifest::new(
        current_binary,
        "self-update",
        latest.to_string(),
    );
    let manifest_note = match manifest.save(&data_dir) {
        Ok(()) => String::new(),
        Err(e) => format!(" (warning: manifest save failed: {e})"),
    };

    Ok(format!(
        "updated: {} -> {}{}",
        CURRENT_VERSION, release.tag_name, manifest_note
    ))
}

fn set_executable(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| CliError::update(format!("chmod failed: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn fsync_file(path: &Path) -> Result<(), CliError> {
    let file = std::fs::File::open(path)
        .map_err(|e| CliError::update(format!("fsync open {}: {e}", path.display())))?;
    file.sync_all()
        .map_err(|e| CliError::update(format!("fsync {}: {e}", path.display())))?;
    Ok(())
}

fn fsync_parent_dir(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            let dir = std::fs::File::open(parent).map_err(|e| {
                CliError::update(format!("fsync dir open {}: {e}", parent.display()))
            })?;
            dir.sync_all()
                .map_err(|e| CliError::update(format!("fsync dir {}: {e}", parent.display())))?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn rollback_path(binary: &Path) -> PathBuf {
    let parent = binary.parent().unwrap_or_else(|| Path::new("."));
    let stem = binary
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("math_talk_radar");
    parent.join(format!(".{stem}.rollback"))
}
