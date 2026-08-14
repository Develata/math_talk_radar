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

// T2-5: invalid --today format → exit 3 (config error), not silent fallback.
#[tokio::test]
async fn t2_5_invalid_today_exits_3() {
    let server = MockServer::start().await;
    mount_rss_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);
    bin()
        .args([
            "scan",
            "--no-state",
            "--sources",
            config.path().to_str().unwrap(),
            "--today",
            "not-a-date",
        ])
        .assert()
        .failure()
        .code(3);
}

// CLI-20: an invalid --today with an ACTIVE state DB must exit 3 WITHOUT
// mutating the state DB. The pre-fix wiring validated --today AFTER store_scan,
// so the invalid scan had already persisted its events before failing.
// Strategy: fresh DB → invalid scan 2 (exit 3) → valid scan 3. If scan 2
// persisted, scan 3 sees pre-existing events and emits no EventAdded. If scan 2
// did NOT persist (the fix), scan 3 is the first write and emits EventAdded.
#[tokio::test]
async fn cli_20_invalid_today_does_not_mutate_state() {
    let server = MockServer::start().await;
    mount_rss_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);
    let state_dir = tempfile::TempDir::new().expect("state dir");
    let state_db = state_dir.path().join("state.redb");

    // Scan 2: invalid --today on a FRESH state DB. Must exit 3.
    bin()
        .args([
            "scan",
            "--sources",
            config.path().to_str().unwrap(),
            "--state",
            state_db.to_str().unwrap(),
            "--today",
            "not-a-date",
        ])
        .assert()
        .failure()
        .code(3);

    // Scan 3: valid --today on the same DB. If scan 2 persisted, no
    // EventAdded. If scan 2 did NOT persist (fix), EventAdded fires.
    let third = bin()
        .args([
            "scan",
            "--sources",
            config.path().to_str().unwrap(),
            "--state",
            state_db.to_str().unwrap(),
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();
    let third_stdout = String::from_utf8_lossy(&third.get_output().stdout);
    let third_v: serde_json::Value =
        serde_json::from_str(&third_stdout).expect("scan 3 stdout is JSON");
    let changes = third_v["changes"].as_array().expect("changes array");
    let has_added = changes.iter().any(|c| c["kind"] == "event_added");
    assert!(
        has_added,
        "CLI-20: scan 3 must emit event_added (scan 2 did not persist); \
         if absent, scan 2 mutated the DB before failing. changes: {changes:?}"
    );
}

// CLI-10 regression: --max-events caps OUTPUT only. A scan that seeds both
// events (no cap) followed by a scan with --max-events 1 must NOT emit
// EventCancelled for the capped-out event, because it is still alive — just
// not displayed. The pre-fix wiring ran store_scan AFTER truncation, so scan 2
// persisted only the 1 kept event and falsely marked the other as cancelled.
#[tokio::test]
async fn cli_10_max_events_does_not_emit_spurious_event_cancelled() {
    let server = MockServer::start().await;
    mount_rss_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);
    let state_dir = tempfile::TempDir::new().expect("state dir");
    let state_db = state_dir.path().join("state.redb");

    // Scan 1: no cap. Both events are seeded into the state DB.
    bin()
        .args([
            "scan",
            "--sources",
            config.path().to_str().unwrap(),
            "--state",
            state_db.to_str().unwrap(),
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();

    // Scan 2: cap output to 1 event. The capped-out event is still alive, so
    // no EventCancelled should appear. Old code emitted one; fix emits none.
    let second = bin()
        .args([
            "scan",
            "--sources",
            config.path().to_str().unwrap(),
            "--state",
            state_db.to_str().unwrap(),
            "--max-events",
            "1",
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();
    let second_stdout = String::from_utf8_lossy(&second.get_output().stdout);
    let second_v: serde_json::Value =
        serde_json::from_str(&second_stdout).expect("scan 2 stdout is JSON");
    assert_eq!(
        second_v["events"].as_array().unwrap().len(),
        1,
        "--max-events 1 caps the output to one event"
    );
    let changes = second_v["changes"].as_array().expect("changes array");
    let has_cancelled = changes.iter().any(|c| c["kind"] == "event_cancelled");
    assert!(
        !has_cancelled,
        "scan 2 must NOT emit event_cancelled for the capped-out event, but got: {changes:?}"
    );
}

// CLI-10 regression (mode filter): a scan that seeds both events (--mode both)
// followed by --mode recordings (no events match) must NOT emit EventCancelled,
// because the events are still alive — just outside the query window. The
// pre-fix wiring ran store_scan AFTER the mode filter, so the empty recordings
// scan persisted nothing and falsely cancelled both events.
#[tokio::test]
async fn cli_10_mode_filter_does_not_emit_spurious_event_cancelled() {
    let server = MockServer::start().await;
    mount_rss_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);
    let state_dir = tempfile::TempDir::new().expect("state dir");
    let state_db = state_dir.path().join("state.redb");

    // Scan 1: --mode both. Both events are seeded into the state DB.
    bin()
        .args([
            "scan",
            "--sources",
            config.path().to_str().unwrap(),
            "--state",
            state_db.to_str().unwrap(),
            "--mode",
            "both",
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();

    // Scan 2: --mode recordings. The RSS feed has no recordings, so the output
    // is empty — but both events are still alive. No EventCancelled.
    let second = bin()
        .args([
            "scan",
            "--sources",
            config.path().to_str().unwrap(),
            "--state",
            state_db.to_str().unwrap(),
            "--mode",
            "recordings",
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();
    let second_stdout = String::from_utf8_lossy(&second.get_output().stdout);
    let second_v: serde_json::Value =
        serde_json::from_str(&second_stdout).expect("scan 2 stdout is JSON");
    assert!(
        second_v["events"].as_array().unwrap().is_empty(),
        "--mode recordings on a feed with no recordings yields empty output"
    );
    let changes = second_v["changes"].as_array().expect("changes array");
    let has_cancelled = changes.iter().any(|c| c["kind"] == "event_cancelled");
    assert!(
        !has_cancelled,
        "scan 2 must NOT emit event_cancelled for events filtered out by mode, but got: {changes:?}"
    );
}

// Feed with titles that match the curated topic registry (config/topics.toml)
// and the curated scholar registry (config/scholars.toml). Used by CORE-11/12
// regression tests to verify the matchers run end-to-end in the scan pipeline.
const TOPIC_SCHOLAR_FEED_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
<channel>
<title>Math Conferences</title>
<link>{base}</link>
<description>Math events</description>
<item><title>Workshop on Arithmetic Geometry and Shimura Varieties</title><link>{base}/detail/ag</link><pubDate>Mon, 01 Sep 2026 00:00:00 +0000</pubDate></item>
<item><title>Lecture by Pierre Deligne on Number Theory</title><link>{base}/detail/deligne</link><pubDate>Tue, 02 Sep 2026 00:00:00 +0000</pubDate></item>
<item><title>Generic talk with no topic or scholar</title><link>{base}/detail/generic</link><pubDate>Wed, 03 Sep 2026 00:00:00 +0000</pubDate></item>
</channel>
</rss>
"#;

async fn mount_topic_scholar_feed(server: &MockServer) {
    let base = server.uri();
    let body = TOPIC_SCHOLAR_FEED_TEMPLATE.replace("{base}", &base);
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
    for slug in ["ag", "deligne", "generic"] {
        Mock::given(method("GET"))
            .and(path(format!("/detail/{slug}")))
            .respond_with(ResponseTemplate::new(200))
            .mount(server)
            .await;
    }
}

// CORE-11: topic matcher must run in the scan pipeline and populate
// event.topics. Before the fix, all adapters set topics: Vec::new() and the
// 30-point topic component was always zero. After the fix, the event titled
// "Workshop on Arithmetic Geometry and Shimura Varieties" must have at least
// the arithmetic_geometry topic in its topics array and a non-zero topic score
// component.
#[tokio::test]
async fn core_11_topic_matching_fires_in_scan() {
    let server = MockServer::start().await;
    mount_topic_scholar_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);

    let output = bin()
        .args([
            "scan",
            "--no-state",
            "--sources",
            config.path().to_str().unwrap(),
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    let events = v["events"].as_array().expect("events array");

    // Find the arithmetic geometry workshop by title.
    let ag_event = events
        .iter()
        .find(|e| {
            e["title"]
                .as_str()
                .unwrap_or("")
                .contains("Arithmetic Geometry")
        })
        .expect("arithmetic geometry event present");
    let topics = ag_event["topics"].as_array().expect("topics array");
    assert!(
        !topics.is_empty(),
        "CORE-11: event.topics must be populated by the topic matcher, got empty array"
    );
    assert!(
        topics
            .iter()
            .any(|t| t["topic_id"] == "arithmetic_geometry"),
        "CORE-11: topics must contain arithmetic_geometry, got: {topics:?}"
    );
    // The topic score component must be non-zero (each matched topic ≥ 8).
    let topic_score = ag_event["score_components"]["topic"].as_u64().unwrap_or(0);
    assert!(
        topic_score > 0,
        "CORE-11: score_components.topic must be > 0, got {topic_score}"
    );
}

// CORE-12: scholar matcher must run in the scan pipeline. The event titled
// "Lecture by Pierre Deligne on Number Theory" must produce a PersonHit with
// the "fields"/"wolf"/"abel"/"crafoord" scholar tags, so the people component
// can recognize the laureate. Before the fix, scholar_tags was always empty
// and the people component never exceeded the 3-point baseline.
#[tokio::test]
async fn core_12_scholar_matching_fires_in_scan() {
    let server = MockServer::start().await;
    mount_topic_scholar_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);

    let output = bin()
        .args([
            "scan",
            "--no-state",
            "--sources",
            config.path().to_str().unwrap(),
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    let events = v["events"].as_array().expect("events array");

    let deligne_event = events
        .iter()
        .find(|e| e["title"].as_str().unwrap_or("").contains("Deligne"))
        .expect("Deligne event present");
    let people = deligne_event["people"].as_array().expect("people array");
    assert!(
        !people.is_empty(),
        "CORE-12: event.people must be populated by the scholar matcher, got empty array"
    );
    let deligne_hit = people
        .iter()
        .find(|p| {
            p["canonical_name"]
                .as_str()
                .unwrap_or("")
                .contains("Deligne")
        })
        .expect("Deligne PersonHit present");
    let tags = deligne_hit["scholar_tags"]
        .as_array()
        .expect("scholar_tags array");
    assert!(
        !tags.is_empty(),
        "CORE-12: scholar_tags must be populated from the registry, got empty array"
    );
    assert!(
        tags.iter().any(|t| t.as_str() == Some("fields")),
        "CORE-12: scholar_tags must contain 'fields', got: {tags:?}"
    );
}

// CORE-11/12 regression guard: the generic event with no topic or scholar in
// its title must have empty topics and empty people, proving the matchers do
// not produce false positives.
#[tokio::test]
async fn core_11_12_no_false_positives_on_generic_title() {
    let server = MockServer::start().await;
    mount_topic_scholar_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);

    let output = bin()
        .args([
            "scan",
            "--no-state",
            "--sources",
            config.path().to_str().unwrap(),
            "--today",
            "2026-08-13",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    let events = v["events"].as_array().expect("events array");

    let generic = events
        .iter()
        .find(|e| e["title"].as_str().unwrap_or("").contains("Generic"))
        .expect("generic event present");
    let topics = generic["topics"].as_array().expect("topics array");
    let people = generic["people"].as_array().expect("people array");
    assert!(
        topics.is_empty(),
        "CORE-11 guard: generic title must produce no topic matches, got: {topics:?}"
    );
    assert!(
        people.is_empty(),
        "CORE-12 guard: generic title must produce no scholar matches, got: {people:?}"
    );
}

// CLI-23: scan piped to a closed stdout must exit 0, not panic exit 101. The
// pre-fix println! panicked with "failed printing to stdout: Broken pipe" and
// exited 101 (not in the §32 contract). The fix uses write_all and treats
// BrokenPipe as a clean exit — the downstream consumer is done, so we stop.
#[tokio::test]
async fn cli_23_broken_pipe_exits_zero() {
    let server = MockServer::start().await;
    mount_rss_feed(&server).await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);

    let bin_path = assert_cmd::cargo::cargo_bin("math_talk_radar");
    let mut cmd = std::process::Command::new(bin_path);
    cmd.args(["scan", "--no-state", "--sources"])
        .arg(config.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    // Drop the stdout reading end immediately → the pipe closes → the next
    // write gets EPIPE. The scan takes time (mock fetch), so by the time it
    // tries to write JSON the pipe is long closed.
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "CLI-23: scan with closed stdout must exit 0, got code {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "CLI-23: stderr must not contain a panic message, got: {stderr}"
    );
}

// CLI-24: invalid --today must fail (exit 3) before any network fetch. The
// pre-fix wiring validated --today after fetch_all, wasting the request budget
// on a scan whose output would be discarded. The fix parses --today before
// fetch_all, so the mock server's endpoints are never hit.
#[tokio::test]
async fn cli_24_invalid_today_skips_network_fetch() {
    let server = MockServer::start().await;
    // Mount both endpoints with expect(0) — if --today is validated before
    // fetch (the fix), neither is called and server.verify() passes. If
    // --today is validated after fetch (the bug), both are called and
    // server.verify() fails.
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let config = write_sources_config(&[(true, "ok", &format!("{}/feed.xml", server.uri()))]);

    bin()
        .args([
            "scan",
            "--no-state",
            "--sources",
            config.path().to_str().unwrap(),
            "--today",
            "not-a-date",
        ])
        .assert()
        .failure()
        .code(3);

    // Assert neither endpoint was hit.
    server.verify().await;
}
