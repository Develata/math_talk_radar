//! wiremock mock-server tests for the radar-fetch HTTP layer.
//!
//! Covers acceptance cases SRC-006 (crawl depth boundary), SRC-007 (host
//! allowlist / redirect rejection), SRC-008 (request budget), HTTP-001
//! (per-request timeout), HTTP-002 (single retry on 503), HTTP-003 (404 is not
//! retried), and REL-002 (global scan deadline bounds the fetch duration).
//!
//! Tests drive the real async engine against an in-process `wiremock` server.
//! `radar-fetch` cannot depend on `radar-adapters`, so each `fetch_source` test
//! supplies a tiny inline `SourceAdapter` stub — the tests assert fetch
//! behavior, not parsing.

use radar_core::{
    AdapterError, AdapterKind, EventCandidate, EventStub, FetchPlan, FetchedDocument,
    SourceAdapter, SourceEvidence, SourceKind, SourceSpec, SourceStatus, SourceTier,
};
use radar_fetch::{
    FetchClient, FetchError, FetchPolicy, HttpPolicy, RequestBudget, RobotsCache,
    engine::fetch_one, fetch_source,
};
use std::time::{Duration, Instant};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// ---- helpers ---------------------------------------------------------------

/// Mount a fast `200` response for `/robots.txt` as the highest-priority mock.
///
/// `fetch_one` always probes `/robots.txt` once per host (cached afterwards).
/// Mounting this last keeps the probe deterministic so it cannot consume an
/// `up_to_n_times` budget or inherit a content-path delay. Content mocks must
/// therefore be mounted *before* calling this.
async fn mount_robots(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(server)
        .await;
}

/// Number of recorded requests that are not the `/robots.txt` probe.
fn content_requests(reqs: &[Request]) -> usize {
    reqs.iter()
        .filter(|r| r.url.path() != "/robots.txt")
        .count()
}

/// True if any recorded request targeted exactly `target`.
fn hit_path(reqs: &[Request], target: &str) -> bool {
    reqs.iter().any(|r| r.url.path() == target)
}

/// Build a minimal enabled `SourceSpec` pointing at `entrypoint`.
fn make_source(entrypoint: Url, max_depth: u8, request_budget: u32) -> SourceSpec {
    SourceSpec {
        id: "test".to_string(),
        name: "Test".to_string(),
        tier: SourceTier::A,
        kind: SourceKind::RssFeed,
        adapter: AdapterKind::Rss,
        entrypoint: Some(entrypoint),
        allowed_hosts: vec![],
        max_depth,
        request_budget,
        media_strategy: None,
        dynamic: false,
        enabled: true,
        fixture: None,
        selectors: None,
    }
}

/// `FetchPolicy` that allows only the host of `url`.
fn allow_server_host(url: &Url) -> FetchPolicy {
    FetchPolicy {
        allowed_hosts: vec![url.host_str().unwrap_or("").to_string()],
    }
}

// ---- stub adapters ---------------------------------------------------------

/// Discovers a single stub and plans enrichment at depths 1, 2, and 3 against
/// the mock server. Used by SRC-006 to prove plans with `depth > max_depth` are
/// never fetched.
struct DepthAdapter {
    base: Url,
}

impl SourceAdapter for DepthAdapter {
    fn discover(
        &self,
        _doc: &FetchedDocument,
        src: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        Ok(vec![EventStub {
            title: "e1".to_string(),
            url: self.base.join("/list").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: src.id.clone(),
                source_url: self.base.clone(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        }])
    }

    fn plan_enrichment(&self, _ev: &EventStub, _src: &SourceSpec) -> Vec<FetchPlan> {
        vec![
            FetchPlan {
                url: self.base.join("/depth1").unwrap(),
                depth: 1,
                reason: "d1".to_string(),
            },
            FetchPlan {
                url: self.base.join("/depth2").unwrap(),
                depth: 2,
                reason: "d2".to_string(),
            },
            FetchPlan {
                url: self.base.join("/depth3").unwrap(),
                depth: 3,
                reason: "d3".to_string(),
            },
        ]
    }

    fn enrich(
        &self,
        _ev: EventStub,
        _docs: &[FetchedDocument],
        _src: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        Err(AdapterError::Parse {
            source_id: String::new(),
            message: "stub".to_string(),
        })
    }
}

/// Discovers `count` stubs, each with one depth-1 enrichment plan at a distinct
/// path. Used by SRC-008 to prove budget exhaustion halts further requests.
struct MultiStubAdapter {
    base: Url,
    count: usize,
}

impl SourceAdapter for MultiStubAdapter {
    fn discover(
        &self,
        _doc: &FetchedDocument,
        src: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        Ok((0..self.count)
            .map(|i| EventStub {
                title: format!("e{i}"),
                url: self.base.join(&format!("/detail{i}")).unwrap(),
                date_hint: None,
                source: SourceEvidence {
                    source_id: src.id.clone(),
                    source_url: self.base.clone(),
                    evidence: None,
                    captured_at: None,
                    native_id: None,
                },
            })
            .collect())
    }

    fn plan_enrichment(&self, ev: &EventStub, _src: &SourceSpec) -> Vec<FetchPlan> {
        vec![FetchPlan {
            url: ev.url.clone(),
            depth: 1,
            reason: "detail".to_string(),
        }]
    }

    fn enrich(
        &self,
        _ev: EventStub,
        _docs: &[FetchedDocument],
        _src: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        Err(AdapterError::Parse {
            source_id: String::new(),
            message: "stub".to_string(),
        })
    }
}

// ---- SRC-006: depth <= 2 (crawl boundary) ----------------------------------

#[tokio::test]
async fn src006_depth_boundary_skips_deep_plans() {
    let server = MockServer::start().await;
    // Every content path returns 200 fast.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    mount_robots(&server).await;

    let entrypoint: Url = server.uri().parse().unwrap();
    let source = make_source(entrypoint.clone(), 2, 20);
    let adapter = DepthAdapter { base: entrypoint };
    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let robots = RobotsCache::new();

    let result = fetch_source(&client, &source, &adapter, &robots, None).await;

    let reqs = server.received_requests().await.expect("requests recorded");
    // Budget bound: total <= request_budget + 1 (the +1 is the robots probe).
    assert!(
        reqs.len() <= source.request_budget as usize + 1,
        "total requests ({}) must be <= budget + 1",
        reqs.len()
    );
    // Depth boundary: depth-1 and depth-2 fetched, depth-3 (exceeds max_depth) is not.
    assert!(hit_path(&reqs, "/depth1"), "depth-1 plan must be fetched");
    assert!(hit_path(&reqs, "/depth2"), "depth-2 plan must be fetched");
    assert!(
        !hit_path(&reqs, "/depth3"),
        "depth-3 plan exceeds max_depth and must not be fetched"
    );
    assert_eq!(result.health.status, SourceStatus::Ok);
}

// ---- SRC-007: host allowlist / redirect to disallowed host -----------------

#[tokio::test]
async fn src007_redirect_to_disallowed_host_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "https://evil.com/"))
        .mount(&server)
        .await;
    mount_robots(&server).await;

    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let url: Url = server.uri().parse().unwrap();
    // Allowlist holds only the mock server's host; the evil.com redirect is rejected.
    let fetch_policy = FetchPolicy {
        allowed_hosts: vec!["127.0.0.1".to_string()],
    };
    let http_policy = client.policy();
    let mut budget = RequestBudget::default();
    let robots = RobotsCache::new();

    let result = fetch_one(
        &client,
        &url,
        &fetch_policy,
        &http_policy,
        &mut budget,
        None,
        &robots,
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        matches!(&err, FetchError::RedirectDisallowed),
        "expected RedirectDisallowed, got {err:?}"
    );
}

// ---- SRC-008: request budget caps fetches ----------------------------------

#[tokio::test]
async fn src008_request_budget_caps_fetches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    mount_robots(&server).await;

    let entrypoint: Url = server.uri().parse().unwrap();
    // Budget = 2: the entrypoint plus a single detail fetch. The remaining four
    // planned details hit BudgetExhausted and are skipped without a request.
    let source = make_source(entrypoint.clone(), 2, 2);
    let adapter = MultiStubAdapter {
        base: entrypoint,
        count: 5,
    };
    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let robots = RobotsCache::new();

    let result = fetch_source(&client, &source, &adapter, &robots, None).await;

    let reqs = server.received_requests().await.expect("requests recorded");
    let content = content_requests(&reqs);
    assert!(
        content <= source.request_budget as usize,
        "content requests ({content}) must be <= budget ({})",
        source.request_budget
    );
    assert_eq!(result.health.status, SourceStatus::Ok);
}

// ---- HTTP-001: per-request timeout -----------------------------------------

#[tokio::test]
async fn http001_request_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    mount_robots(&server).await;

    let policy = HttpPolicy {
        request_timeout: Duration::from_secs(2),
        ..Default::default()
    };
    let client = FetchClient::new(policy).unwrap();
    let url: Url = server.uri().parse().unwrap();
    let fetch_policy = allow_server_host(&url);
    let http_policy = client.policy();
    let mut budget = RequestBudget::default();
    let robots = RobotsCache::new();

    let start = Instant::now();
    let result = fetch_one(
        &client,
        &url,
        &fetch_policy,
        &http_policy,
        &mut budget,
        None,
        &robots,
    )
    .await;
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(3),
        "fetch should time out within ~3s, took {elapsed:?}"
    );
    let err = result.unwrap_err();
    // reqwest's per-request timeout surfaces as NetworkError(is_timeout); the
    // FetchError::Timeout variant fires only via the global deadline path.
    let is_timeout = matches!(&err, FetchError::Timeout)
        || matches!(&err, FetchError::NetworkError(e) if e.is_timeout());
    assert!(is_timeout, "expected a timeout-class error, got {err:?}");
}

// ---- HTTP-002: retry once on 503 -------------------------------------------

#[tokio::test]
async fn http002_retry_once_on_503() {
    let server = MockServer::start().await;
    // 503 once, with the highest priority so it is matched before the 200
    // fallback. wiremock checks same-priority mocks in insertion order, so an
    // explicit priority is the only way to guarantee the 503 fires first.
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    // 200 fallback used once the 503 mock is exhausted.
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    mount_robots(&server).await;

    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let url: Url = server.uri().parse().unwrap();
    let fetch_policy = allow_server_host(&url);
    let http_policy = client.policy();
    let mut budget = RequestBudget::default();
    let robots = RobotsCache::new();

    let result = fetch_one(
        &client,
        &url,
        &fetch_policy,
        &http_policy,
        &mut budget,
        None,
        &robots,
    )
    .await;

    assert!(
        result.is_ok(),
        "retry should succeed: {:?}",
        result.as_ref().err()
    );
    let reqs = server.received_requests().await.expect("requests recorded");
    assert_eq!(
        content_requests(&reqs),
        2,
        "expected exactly 2 content requests (503 then 200)"
    );
}

// ---- HTTP-003: 404 is not retried ------------------------------------------

#[tokio::test]
async fn http003_404_no_retry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    mount_robots(&server).await;

    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let url: Url = server.uri().parse().unwrap();
    let fetch_policy = allow_server_host(&url);
    let http_policy = client.policy();
    let mut budget = RequestBudget::default();
    let robots = RobotsCache::new();

    let result = fetch_one(
        &client,
        &url,
        &fetch_policy,
        &http_policy,
        &mut budget,
        None,
        &robots,
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        matches!(&err, FetchError::HttpError { status: 404 }),
        "expected HttpError{{404}}, got {err:?}"
    );
    let reqs = server.received_requests().await.expect("requests recorded");
    assert_eq!(content_requests(&reqs), 1, "404 must not be retried");
}

// ---- REL-002: global deadline bounds fetch duration ------------------------

#[tokio::test]
async fn rel002_global_deadline_bounds_fetch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    mount_robots(&server).await;

    let entrypoint: Url = server.uri().parse().unwrap();
    let source = make_source(entrypoint, 2, 20);
    let adapter = MultiStubAdapter {
        base: server.uri().parse().unwrap(),
        count: 1,
    };
    let client = FetchClient::new(HttpPolicy::default()).unwrap();
    let robots = RobotsCache::new();
    let deadline = Some(Instant::now() + Duration::from_secs(2));

    let start = Instant::now();
    let result = fetch_source(&client, &source, &adapter, &robots, deadline).await;
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(3),
        "global deadline should bound the fetch within ~3s, took {elapsed:?}"
    );
    // The deadline-derived per-request timeout surfaces as HttpError (reqwest
    // timeout -> NetworkError -> HttpError); Timeout fires only when the
    // deadline is already past at the start of a fetch_one call.
    assert!(
        matches!(
            result.health.status,
            SourceStatus::Timeout | SourceStatus::HttpError
        ),
        "expected Timeout or HttpError (deadline-induced), got {:?}",
        result.health.status
    );
}
