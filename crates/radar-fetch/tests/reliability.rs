//! Reliability tests for `radar-fetch` (REL-001 fault isolation, §32; robots
//! de-dup, Oracle #2).
//!
//! wiremock-backed integration tests: each test spins up an in-process mock
//! HTTP server and drives [`radar_fetch::fetch_all`] against it. No real
//! network is touched.

use radar_core::config::{AdapterKind, SourceKind, SourceTier};
use radar_core::{
    AdapterError, EventCandidate, EventStub, FetchPlan, FetchedDocument, SourceAdapter, SourceSpec,
    SourceStatus,
};
use radar_fetch::{FetchClient, HttpPolicy, fetch_all};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const SUCCESS_BODY: &str = "<html><body>ok</body></html>";
const ROBOTS_BODY: &str = "User-agent: *\nDisallow: /private\n";

/// Stub adapter mirroring the `http_mock` test pattern: empty `discover`,
/// empty `plan_enrichment`, erroring `enrich`. Because `discover` returns no
/// stubs, `enrich` is never invoked — the error is just a sentinel.
struct StubAdapter;

impl SourceAdapter for StubAdapter {
    fn discover(
        &self,
        _document: &FetchedDocument,
        _source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        Ok(Vec::new())
    }

    fn plan_enrichment(&self, _event: &EventStub, _source: &SourceSpec) -> Vec<FetchPlan> {
        Vec::new()
    }

    fn enrich(
        &self,
        _event: EventStub,
        _documents: &[FetchedDocument],
        _source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        Err(AdapterError::Parse {
            source_id: "stub".to_string(),
            message: "stub enrich is not implemented".to_string(),
        })
    }
}

/// Build an enabled `SourceSpec` whose entrypoint is `{server_uri}/source_{i}`.
fn make_source(i: usize, server_uri: &str) -> SourceSpec {
    SourceSpec {
        id: format!("src-{i}"),
        name: format!("Source {i}"),
        tier: SourceTier::A,
        kind: SourceKind::RssFeed,
        adapter: AdapterKind::Rss,
        entrypoint: Some(format!("{server_uri}/source_{i}").parse().unwrap()),
        allowed_hosts: Vec::new(),
        max_depth: 2,
        request_budget: 20,
        media_strategy: None,
        dynamic: false,
        enabled: true,
        fixture: None,
        selectors: None,
    }
}

/// REL-001 (§32): a failing source must not abort the whole scan. 10 sources
/// share one mock server; 3 return HTTP 500. `fetch_all` must still return a
/// result for every source, isolating the failures as `SourceStatus::HttpError`
/// while the 7 successful sources complete as `SourceStatus::Ok`.
#[tokio::test]
async fn rel_001_fault_isolation_does_not_abort_scan() {
    let server = MockServer::start().await;
    let uri = server.uri();

    // Shared robots mock — all sources share one host, so the RobotsCache
    // fetches this exactly once (verified separately in the de-dup test).
    Mock::given(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ROBOTS_BODY))
        .mount(&server)
        .await;

    // 10 sources on distinct paths: indices 0..6 return 200, 7..9 return 500.
    for i in 0..10usize {
        let status = if i >= 7 { 500 } else { 200 };
        Mock::given(path(format!("/source_{i}")))
            .respond_with(ResponseTemplate::new(status).set_body_string(SUCCESS_BODY))
            .mount(&server)
            .await;
    }

    let sources: Vec<SourceSpec> = (0..10).map(|i| make_source(i, &uri)).collect();
    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let results = fetch_all(&client, &sources, None, |_| Box::new(StubAdapter)).await;

    // §32: fetch_all returns a result for every source — it does not abort on
    // individual source failures.
    assert_eq!(
        results.len(),
        10,
        "fetch_all must return a result for all 10 sources",
    );

    let mut ok_ids: Vec<String> = Vec::new();
    let mut err_ids: Vec<String> = Vec::new();
    for r in &results {
        // The stub adapter discovers nothing, so candidates are always empty;
        // failed sources never reach the adapter at all.
        assert!(
            r.candidates.is_empty(),
            "source {} had non-empty candidates",
            r.health.source,
        );
        match r.health.status {
            SourceStatus::Ok => ok_ids.push(r.health.source.clone()),
            SourceStatus::HttpError => err_ids.push(r.health.source.clone()),
            other => panic!("unexpected status {other:?} for source {}", r.health.source),
        }
    }
    assert_eq!(ok_ids.len(), 7, "expected 7 successful sources (HTTP 200)");
    assert_eq!(err_ids.len(), 3, "expected 3 failed sources (HTTP 500)");

    // The 3 failures must be exactly the mocked-500 sources — proving the 200
    // sources completed normally alongside them (fault isolation).
    err_ids.sort();
    assert_eq!(
        err_ids,
        vec![
            "src-7".to_string(),
            "src-8".to_string(),
            "src-9".to_string()
        ],
        "exactly src-7, src-8, src-9 must be the failed sources",
    );
}

/// Oracle #2: the robots cache de-dups robots.txt fetches per host. Multiple
/// sources on the same host share a single `RobotsCache`, so `/robots.txt` is
/// fetched exactly once even though every source consults it.
#[tokio::test]
async fn robots_txt_fetched_once_per_host() {
    let server = MockServer::start().await;
    let uri = server.uri();

    Mock::given(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ROBOTS_BODY))
        .mount(&server)
        .await;

    let n = 5usize;
    for i in 0..n {
        Mock::given(path(format!("/source_{i}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(SUCCESS_BODY))
            .mount(&server)
            .await;
    }

    let sources: Vec<SourceSpec> = (0..n).map(|i| make_source(i, &uri)).collect();
    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let _results = fetch_all(&client, &sources, None, |_| Box::new(StubAdapter)).await;

    // OnceCell de-dup: all sources on the same host share one robots fetch.
    let requests = server
        .received_requests()
        .await
        .expect("mock server records requests by default");
    let robots_count = requests
        .iter()
        .filter(|r| r.url.path() == "/robots.txt")
        .count();
    assert_eq!(
        robots_count, 1,
        "robots.txt must be fetched exactly once per host (OnceCell de-dup)",
    );
}
