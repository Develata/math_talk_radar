//! M5 lifecycle sandbox tests: UPD-001..004, UNS-001..004.
//!
//! All tests run in a temporary sandbox. The real install is never touched.
//! UPD tests use wiremock to serve a fake release API; UNS tests set XDG env
//! vars to temp dirs so only sandbox paths are visible.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BINARY_ASSET_NAME: &str = "math_talk_radar-x86_64-unknown-linux-musl";
const CHECKSUM_ASSET_NAME: &str = "math_talk_radar-x86_64-unknown-linux-musl.sha256";
const WRONG_CHECKSUM: &str = "0000000000000000000000000000000000000000000000000000000000000000";

const VALID_SCRIPT: &[u8] = b"#!/bin/sh\nexit 0\n";
const BROKEN_SCRIPT: &[u8] = b"#!/bin/sh\nexit 1\n";

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(s, "{byte:02x}");
    }
    s
}

fn bin() -> Command {
    Command::cargo_bin("math_talk_radar").expect("binary present")
}

struct Sandbox {
    _tmp: TempDir,
    root: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = TempDir::new().expect("temp dir");
        let root = tmp.path().to_path_buf();
        Self { _tmp: tmp, root }
    }

    fn xdg_data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn xdg_config(&self) -> PathBuf {
        self.root.join("config")
    }

    fn xdg_cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    fn data_dir(&self) -> PathBuf {
        self.xdg_data().join("math_talk_radar")
    }

    fn config_dir(&self) -> PathBuf {
        self.xdg_config().join("math_talk_radar")
    }

    fn cache_dir(&self) -> PathBuf {
        self.xdg_cache().join("math_talk_radar")
    }

    fn create_fake_binary(&self, content: &[u8]) -> PathBuf {
        let bin_dir = self.root.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let binary = bin_dir.join("math_talk_radar");
        std::fs::write(&binary, content).expect("write binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
        binary
    }

    fn write_manifest(&self, binary_path: &Path) {
        std::fs::create_dir_all(self.data_dir()).expect("create data dir");
        let manifest = math_talk_radar_cli::lifecycle::manifest::InstallManifest::new(
            binary_path.to_path_buf(),
            "self-update",
            "0.1.0",
        );
        manifest.save(&self.data_dir()).expect("save manifest");
    }

    fn setup_full(&self, binary_content: &[u8]) -> PathBuf {
        let binary = self.create_fake_binary(binary_content);
        self.write_manifest(&binary);
        std::fs::create_dir_all(self.config_dir()).expect("create config dir");
        std::fs::create_dir_all(self.cache_dir()).expect("create cache dir");
        std::fs::create_dir_all(self.data_dir()).expect("create data dir");
        binary
    }

    fn set_env(&self, cmd: &mut Command) {
        cmd.env("XDG_DATA_HOME", self.xdg_data())
            .env("XDG_CONFIG_HOME", self.xdg_config())
            .env("XDG_CACHE_HOME", self.xdg_cache());
    }
}

async fn mount_release(
    server: &MockServer,
    tag: &str,
    binary_bytes: &[u8],
    checksum_override: Option<&str>,
) {
    let binary_url = format!("{}/download/binary", server.uri());
    let checksum_url = format!("{}/download/checksum", server.uri());

    let release_json = serde_json::json!({
        "tag_name": tag,
        "assets": [
            {"name": BINARY_ASSET_NAME, "browser_download_url": binary_url},
            {"name": CHECKSUM_ASSET_NAME, "browser_download_url": checksum_url},
        ]
    });

    Mock::given(method("GET"))
        .and(path("/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(release_json))
        .mount(server)
        .await;

    let binary_str = std::str::from_utf8(binary_bytes).expect("script is UTF-8");
    Mock::given(method("GET"))
        .and(path("/download/binary"))
        .respond_with(ResponseTemplate::new(200).set_body_string(binary_str))
        .mount(server)
        .await;

    let checksum_body = checksum_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| sha256_hex(binary_bytes));
    Mock::given(method("GET"))
        .and(path("/download/checksum"))
        .respond_with(ResponseTemplate::new(200).set_body_string(checksum_body))
        .mount(server)
        .await;
}

// UPD-001: update --check writes nothing.
#[tokio::test]
async fn upd_001_check_no_write() {
    let server = MockServer::start().await;
    mount_release(&server, "v99.0.0", VALID_SCRIPT, None).await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);
    let content_before = std::fs::read(&binary).expect("read binary before");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update", "--check"])
        .assert()
        .success();

    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_before, content_after,
        "update --check must not modify the binary"
    );
}

// UPD-002: checksum failure preserves the working binary.
#[tokio::test]
async fn upd_002_checksum_failure_preserves_binary() {
    let server = MockServer::start().await;
    mount_release(&server, "v99.0.0", VALID_SCRIPT, Some(WRONG_CHECKSUM)).await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);
    let content_before = std::fs::read(&binary).expect("read binary before");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update"])
        .assert()
        .failure()
        .code(10);

    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_before, content_after,
        "binary must be unchanged after checksum failure"
    );
}

// UPD-003: valid update atomically replaces the binary.
#[tokio::test]
async fn upd_003_valid_update_replaces_binary() {
    let server = MockServer::start().await;
    let original = b"#!/bin/sh\necho old\nexit 0\n";
    mount_release(&server, "v99.0.0", VALID_SCRIPT, None).await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(original);

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update"])
        .assert()
        .success();

    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_after, VALID_SCRIPT,
        "binary must be replaced with the downloaded content"
    );
}

// R9-M07: a successful update must RETAIN the rollback copy (Disposition A).
// The previous binary stays at `.<stem>.rollback` alongside the current
// binary, overwritten by the next successful update. This guarantees a
// manual-recovery path to the last-known-good version if the new binary
// fails at runtime (a defect the self-test cannot catch).
#[tokio::test]
async fn r9_m07_rollback_retained_after_successful_update() {
    let server = MockServer::start().await;
    let original = b"#!/bin/sh\necho old\nexit 0\n";
    mount_release(&server, "v99.0.0", VALID_SCRIPT, None).await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(original);

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update"])
        .assert()
        .success();

    // The rollback file must exist alongside the binary.
    let rollback = binary
        .parent()
        .expect("binary has parent")
        .join(".math_talk_radar.rollback");
    assert!(
        rollback.exists(),
        "rollback copy must be retained after successful update: {}",
        rollback.display()
    );
    let rollback_content = std::fs::read(&rollback).expect("read rollback");
    assert_eq!(
        rollback_content, original,
        "rollback must contain the PREVIOUS binary content"
    );
    // The current binary must have the NEW content.
    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_after, VALID_SCRIPT,
        "binary must be the new version"
    );
}

// UPD-004: broken candidate triggers rollback (original preserved).
#[tokio::test]
async fn upd_004_broken_candidate_preserves_binary() {
    let server = MockServer::start().await;
    mount_release(&server, "v99.0.0", BROKEN_SCRIPT, None).await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);
    let content_before = std::fs::read(&binary).expect("read binary before");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update"])
        .assert()
        .failure()
        .code(10);

    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_before, content_after,
        "binary must be unchanged after broken candidate"
    );
}

// R9-H11: an oversized checksum download must be rejected before the binary
// is touched. MAX_CHECKSUM_BYTES is 1024; serve 2 KiB and assert exit 10
// with the working binary unchanged. This exercises both the Content-Length
// pre-check and the post-download body-size guard (wiremock sets the header).
#[tokio::test]
async fn r9_h11_oversized_checksum_rejected() {
    let server = MockServer::start().await;
    let oversize = "a".repeat(2048);
    mount_release(&server, "v99.0.0", VALID_SCRIPT, Some(&oversize)).await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);
    let content_before = std::fs::read(&binary).expect("read binary before");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update"])
        .assert()
        .failure()
        .code(10);

    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_before, content_after,
        "binary must be unchanged when checksum download exceeds size limit"
    );
}

// R9-H11: a redirect from a whitelisted download host to an off-whitelist
// host must be rejected rather than followed. /download/binary returns 302
// to https://evil.example.com/binary; send_validated re-validates the
// Location host against DOWNLOAD_HOSTS and rejects it before any request to
// evil.example.com. Update fails with exit 10 and the working binary is
// unchanged.
#[tokio::test]
async fn r9_h11_redirect_to_off_whitelist_host_rejected() {
    let server = MockServer::start().await;
    let binary_url = format!("{}/download/binary", server.uri());
    let checksum_url = format!("{}/download/checksum", server.uri());

    let release_json = serde_json::json!({
        "tag_name": "v99.0.0",
        "assets": [
            {"name": BINARY_ASSET_NAME, "browser_download_url": binary_url},
            {"name": CHECKSUM_ASSET_NAME, "browser_download_url": checksum_url},
        ]
    });
    Mock::given(method("GET"))
        .and(path("/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(release_json))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download/checksum"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sha256_hex(VALID_SCRIPT)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download/binary"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "https://evil.example.com/binary"),
        )
        .mount(&server)
        .await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);
    let content_before = std::fs::read(&binary).expect("read binary before");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update"])
        .assert()
        .failure()
        .code(10);

    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_before, content_after,
        "binary must be unchanged when redirect targets an off-whitelist host"
    );
}

// R9-H11: a relative redirect within the same (whitelisted) host must be
// followed. /download/binary returns 302 to /download/binary-actual, which
// serves the real binary. The updater resolves the relative Location, re-
// validates the host, follows, and completes the update normally.
#[tokio::test]
async fn r9_h11_relative_redirect_within_whitelist_followed() {
    let server = MockServer::start().await;
    let binary_url = format!("{}/download/binary", server.uri());
    let checksum_url = format!("{}/download/checksum", server.uri());

    let release_json = serde_json::json!({
        "tag_name": "v99.0.0",
        "assets": [
            {"name": BINARY_ASSET_NAME, "browser_download_url": binary_url},
            {"name": CHECKSUM_ASSET_NAME, "browser_download_url": checksum_url},
        ]
    });
    Mock::given(method("GET"))
        .and(path("/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(release_json))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download/checksum"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sha256_hex(VALID_SCRIPT)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download/binary"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "/download/binary-actual"),
        )
        .mount(&server)
        .await;

    let binary_str = std::str::from_utf8(VALID_SCRIPT).expect("script is UTF-8");
    Mock::given(method("GET"))
        .and(path("/download/binary-actual"))
        .respond_with(ResponseTemplate::new(200).set_body_string(binary_str))
        .mount(&server)
        .await;

    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(b"#!/bin/sh\necho old\nexit 0\n");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.env("MATH_TALK_RADAR_RELEASE_API", server.uri())
        .args(["update"])
        .assert()
        .success();

    let content_after = std::fs::read(&binary).expect("read binary after");
    assert_eq!(
        content_after, VALID_SCRIPT,
        "binary must be replaced after following a whitelisted redirect"
    );
}

// UNS-001: dry-run mutates nothing.
#[test]
fn uns_001_dry_run_zero_mutation() {
    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.args(["uninstall", "--dry-run", "--keep-data"])
        .assert()
        .success();

    assert!(binary.exists(), "binary must exist after dry-run");
    assert!(
        sandbox.config_dir().exists(),
        "config dir must exist after dry-run"
    );
    assert!(
        sandbox.cache_dir().exists(),
        "cache dir must exist after dry-run"
    );
    assert!(
        sandbox.data_dir().exists(),
        "data dir must exist after dry-run"
    );
}

// UNS-002: keep-data preserves only data.
#[test]
fn uns_002_keep_data_preserves_data() {
    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.args(["uninstall", "--keep-data", "--yes"])
        .assert()
        .success();

    assert!(!binary.exists(), "binary must be deleted");
    assert!(!sandbox.config_dir().exists(), "config dir must be deleted");
    assert!(!sandbox.cache_dir().exists(), "cache dir must be deleted");
    assert!(sandbox.data_dir().exists(), "data dir must be preserved");
}

// UNS-003: purge removes all app-owned paths.
#[test]
fn uns_003_purge_removes_all() {
    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.args(["uninstall", "--purge", "--yes"])
        .assert()
        .success();

    assert!(!binary.exists(), "binary must be deleted");
    assert!(!sandbox.config_dir().exists(), "config dir must be deleted");
    assert!(!sandbox.cache_dir().exists(), "cache dir must be deleted");
    assert!(!sandbox.data_dir().exists(), "data dir must be deleted");
}

// UNS-004: unmanaged binary is protected without --force-unmanaged.
#[test]
fn uns_004_unmanaged_binary_protected() {
    let sandbox = Sandbox::new();
    // No manifest → binary_path falls back to current_exe (under target/debug/).
    // is_unmanaged_binary returns true → uninstall refuses without --force-unmanaged.

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.args(["uninstall", "--keep-data", "--yes"])
        .assert()
        .failure()
        .code(11);
}

// UNS-005: stale manifest (recorded path gone) must not bypass dev-binary
// protection. binary_path() falls back to current_exe() (under target/), and
// the manifest no longer backs it — refuse without --force-unmanaged.
#[test]
fn uns_005_stale_manifest_protects_dev_binary() {
    let sandbox = Sandbox::new();
    let gone_binary = sandbox.root.join("bin").join("math_talk_radar");
    sandbox.write_manifest(&gone_binary);
    std::fs::create_dir_all(sandbox.config_dir()).expect("create config dir");
    std::fs::create_dir_all(sandbox.cache_dir()).expect("create cache dir");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.args(["uninstall", "--keep-data", "--yes"])
        .assert()
        .failure()
        .code(11);
}

// R9-H12: uninstall must refuse while an update lock is held. A concurrent
// update could be mid-rename; deleting the binary underneath it would leave
// update's self-test running against a deleted path. The lock file is
// pre-created with PID 1 (init, always alive) and starttime 0 (treated as
// "alive, do not steal"), so the stale-recovery path does not reclaim it.
// Uninstall must fail with exit 11 and leave the binary intact.
#[test]
fn r9_h12_uninstall_refuses_while_update_lock_held() {
    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);

    std::fs::create_dir_all(sandbox.data_dir()).expect("create data dir");
    std::fs::write(sandbox.data_dir().join("update.lock"), "1:0:12345").expect("write update.lock");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.args(["uninstall", "--keep-data", "--yes"])
        .assert()
        .failure()
        .code(11);

    assert!(
        binary.exists(),
        "binary must NOT be deleted while lock is held"
    );
    assert!(
        sandbox.config_dir().exists(),
        "config dir must NOT be deleted while lock is held"
    );
}

// R9-B07 / R9-H12: uninstall must NOT follow a symlink sibling planted
// alongside the binary. The prefix-based sibling-deletion loop scans the
// binary's parent for `.math_talk_radar.update.*` and `.math_talk_radar.rollback*`
// names; a symlink with that name pointing outside the directory must be
// skipped (not canonicalized-and-deleted). The sentinel the symlink targets
// must remain untouched.
#[cfg(unix)]
#[test]
fn r9_b07_uninstall_skips_symlink_sibling() {
    use std::os::unix::fs::symlink;
    let sandbox = Sandbox::new();
    let binary = sandbox.setup_full(VALID_SCRIPT);

    // Plant a symlink sibling named like a retained rollback (M07 retention
    // leaves `.math_talk_radar.rollback` after a prior update). Point it at
    // a sentinel file outside the binary's parent dir.
    let sentinel_dir = tempfile::tempdir().expect("sentinel dir");
    let sentinel = sentinel_dir.path().join("sentinel.txt");
    std::fs::write(&sentinel, b"SENTINEL-UNINSTALL").expect("write sentinel");
    let rollback_link = binary
        .parent()
        .expect("binary has parent")
        .join(".math_talk_radar.rollback");
    symlink(&sentinel, &rollback_link).expect("plant symlink sibling");

    let mut cmd = bin();
    sandbox.set_env(&mut cmd);
    cmd.args(["uninstall", "--purge", "--yes"])
        .assert()
        .success();

    // The symlink sibling must NOT be deleted (it was skipped, not followed).
    let meta = std::fs::symlink_metadata(&rollback_link);
    assert!(
        meta.is_ok(),
        "symlink sibling must NOT be deleted by uninstall: {:?}",
        meta.err()
    );
    assert!(
        meta.unwrap().is_symlink(),
        "sibling must still be a symlink (not its target)"
    );
    // The sentinel must be untouched.
    let sentinel_after = std::fs::read(&sentinel).expect("read sentinel");
    assert_eq!(
        sentinel_after, b"SENTINEL-UNINSTALL",
        "symlink target must NOT be followed/deleted by uninstall"
    );
    // The binary itself must be deleted (uninstall proceeded past the symlink).
    assert!(!binary.exists(), "binary must still be deleted");
}
