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

fn http_client() -> Result<reqwest::Client, CliError> {
    reqwest::Client::builder()
        .user_agent(RELEASE_USER_AGENT)
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

async fn download_bytes(url: &str) -> Result<Vec<u8>, CliError> {
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
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| CliError::update(format!("download body read failed: {e}")))
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
    let mut file = std::fs::File::create(dest)
        .map_err(|e| CliError::update(format!("create temp file failed: {e}")))?;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| CliError::update(format!("download stream error: {e}")))?;
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
    let current_binary = paths::binary_path(&paths::data_dir())
        .ok_or_else(|| CliError::update("cannot resolve current binary path"))?;
    if !force_unmanaged && paths::is_unmanaged_binary(&current_binary) {
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
    let checksum_bytes = download_bytes(&checksum_asset.browser_download_url).await?;
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
    let data_dir = paths::data_dir();
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
