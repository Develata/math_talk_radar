//! M4 CLI integration tests: CLI-001..004, CFG-002, HTTP-004, HTTP-005.
//!
//! Drives the real `math_talk_radar` binary via `assert_cmd`. HTTP-004 uses
//! `wiremock` to serve a valid RSS feed alongside a failing source; CLI-003/004
//! use the same mock to produce real JSON output on stdout.

use assert_cmd::Command;
use tempfile::NamedTempFile;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RSS_FEED_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
<channel>
<title>Math Conferences</title>
<link>{base}</link>
<description>Math events</description>
<item><title>Conference on Algebra</title><link>{base}/detail/algebra</link><pubDate>Mon, 01 Sep 2026 00:00:00 +0000</pubDate></item>
<item><title>Workshop on Graph Theory</title><link>{base}/detail/graph</link><pubDate>Tue, 02 Sep 2026 00:00:00 +0000</pubDate></item>
</channel>
</rss>
"#;

fn write_sources_config(entries: &[(bool, &str, &str)]) -> NamedTempFile {
    let mut toml = String::new();
    for &(enabled, id, entrypoint) in entries {
        toml.push_str(&format!(
            "[[sources]]\nid = \"{id}\"\nname = \"{id}\"\nadapter = \"rss\"\nkind = \"rss_feed\"\nentrypoint = \"{entrypoint}\"\nenabled = {enabled}\nmax_depth = 1\nrequest_budget = 5\n\n"
        ));
    }
    let file = NamedTempFile::new().expect("temp file");
    std::fs::write(file.path(), toml).expect("write config");
    file
}

async fn mount_rss_feed(server: &MockServer) {
    let base = server.uri();
    let body = RSS_FEED_TEMPLATE.replace("{base}", &base);
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/detail/algebra"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/detail/graph"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
}

fn bin() -> Command {
    Command::cargo_bin("math_talk_radar").expect("binary present")
}

// CLI-001: --help lists all 6 subcommands.
#[test]
fn cli_001_help_lists_all_subcommands() {
    let output = bin().arg("--help").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    for cmd in ["scan", "sources", "doctor", "update", "uninstall", "schema"] {
        assert!(
            stdout.contains(cmd),
            "expected --help to mention '{cmd}', got:\n{stdout}"
        );
    }
}

// CLI-002: --version exits 0, no network/state init.
#[test]
fn cli_002_version_exits_zero() {
    bin().arg("--version").assert().success();
}

// CLI-003: scan stdout is pure JSON with schema_version.
#[tokio::test]
async fn cli_003_scan_stdout_is_pure_json() {
    let server = MockServer::start().await;
    mount_rss_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);
    let output = bin()
        .args(["scan", "--no-state", "--sources"])
        .arg(config.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert_eq!(v["schema_version"], "1.0");
    assert!(v["events"].is_array());
    assert!(v["source_health"].is_array());
}

// CLI-004: stderr/stdout are separated — stdout is JSON, stderr has no JSON.
#[tokio::test]
async fn cli_004_stderr_stdout_separated() {
    let server = MockServer::start().await;
    mount_rss_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);
    let output = bin()
        .args(["scan", "--no-state", "--sources"])
        .arg(config.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    serde_json::from_str::<serde_json::Value>(&stdout).expect("stdout is JSON");
    assert!(
        !stderr.contains("\"schema_version\""),
        "stderr must not contain JSON output, got:\n{stderr}"
    );
}

// CFG-002: invalid config fails closed (exit 3).
#[test]
fn cfg_002_invalid_config_fails_closed() {
    let file = NamedTempFile::new().expect("temp file");
    std::fs::write(file.path(), "this is not = valid = toml = [[[").expect("write");
    bin()
        .args(["scan", "--no-state", "--sources"])
        .arg(file.path())
        .assert()
        .failure()
        .code(3);
}

// HTTP-005: zero usable sources → exit 4.
#[test]
fn http_005_zero_usable_sources_exit_4() {
    let config = write_sources_config(&[(false, "disabled", "https://example.com/feed.xml")]);
    bin()
        .args(["scan", "--no-state", "--sources"])
        .arg(config.path())
        .assert()
        .failure()
        .code(4);
}

// HTTP-004: partial source failure → exit 0 with events from the working source.
#[tokio::test]
async fn http_004_partial_failure_exit_0() {
    let ok_server = MockServer::start().await;
    mount_rss_feed(&ok_server).await;
    let fail_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&fail_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&fail_server)
        .await;

    let config = write_sources_config(&[
        (true, "ok", &format!("{}/feed.xml", ok_server.uri())),
        (true, "fail", &format!("{}/feed.xml", fail_server.uri())),
    ]);
    let output = bin()
        .args(["scan", "--no-state", "--sources"])
        .arg(config.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout is JSON despite partial failure");
    let health = v["source_health"].as_array().expect("source_health array");
    assert!(
        health
            .iter()
            .any(|h| h["source"] == "ok" && h["status"] == "ok"),
        "working source must report ok, got: {health:?}"
    );
    assert!(
        health
            .iter()
            .any(|h| h["source"] == "fail" && h["status"] != "ok"),
        "failing source must report non-ok status, got: {health:?}"
    );
}
