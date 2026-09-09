//! Real run_scan pipeline against 20 local RSS sources, including registry
//! enrichment, dedup, ranking, state, filters and public output serialization.
#![forbid(unsafe_code)]
use clap::Parser;
use math_talk_radar_cli::{
    cli::{Cli, Command},
    output, scan_engine,
};
use radar_core::{
    AdapterError, EventCandidate, EventStub, FetchPlan, FetchedDocument, SourceAdapter, SourceSpec,
};
use std::time::Instant;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

struct PanicAdapter;
impl SourceAdapter for PanicAdapter {
    fn discover(
        &self,
        _: &FetchedDocument,
        _: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        panic!("injected release-profile source panic")
    }
    fn plan_enrichment(&self, _: &EventStub, _: &SourceSpec) -> Vec<FetchPlan> {
        vec![]
    }
    fn enrich(
        &self,
        _: EventStub,
        _: &[FetchedDocument],
        _: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        unreachable!("discovery deliberately panics")
    }
}

fn main() {
    // cargo test normally forces unwind even if release is configured to abort.
    // An actual release example is needed to enforce the production invariant.
    const {
        assert!(
            cfg!(panic = "unwind"),
            "source isolation requires release unwinding"
        );
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run());
}

async fn run() {
    let server = MockServer::start().await;
    let base = server.uri();
    Mock::given(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let mut config = String::new();
    for source in 0..20 {
        let mut feed = format!(
            "<rss version=\"2.0\"><channel><title>Math</title><link>{base}</link><description>Offline probe</description>"
        );
        for event in 0..20 {
            let detail = format!("/detail/{source}/{event}");
            feed.push_str(&format!("<item><title>Number theory seminar {source}-{event}</title><link>{base}{detail}</link><pubDate>Tue, 01 Sep 2026 00:00:00 +0000</pubDate></item>"));
            Mock::given(path(detail))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;
        }
        feed.push_str("</channel></rss>");
        Mock::given(path(format!("/feed/{source}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(feed))
            .mount(&server)
            .await;
        config.push_str(&format!("[[sources]]\nid = \"s{source:02}\"\nname = \"Source {source}\"\nadapter = \"rss\"\nkind = \"rss_feed\"\nentrypoint = \"{base}/feed/{source}\"\nenabled = true\nrequest_budget = 21\n\n"));
    }
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("sources.toml");
    let state_path = temp.path().join("state.redb");
    std::fs::write(&config_path, &config).unwrap();
    let arguments = || {
        let cli = Cli::parse_from([
            "math_talk_radar",
            "scan",
            "--sources",
            config_path.to_str().unwrap(),
            "--state",
            state_path.to_str().unwrap(),
            "--today",
            "2026-09-01",
            "--mode",
            "both",
            "--jobs",
            "8",
        ]);
        let Command::Scan(args) = cli.command else {
            unreachable!("fixed CLI args")
        };
        args
    };
    let start = Instant::now();
    let first = scan_engine::run_scan(arguments()).await.unwrap();
    assert_eq!(first.events.len(), 400);
    assert!(
        first
            .source_health
            .iter()
            .all(|h| h.status == radar_core::SourceStatus::Ok),
        "source health: {:?}",
        first.source_health
    );
    let second = scan_engine::run_scan(arguments()).await.unwrap();
    assert_eq!(second.events.len(), 400);
    assert!(
        second
            .source_health
            .iter()
            .all(|h| h.status == radar_core::SourceStatus::Ok),
        "second scan source health: {:?}",
        second.source_health
    );
    assert!(second.changes.is_empty());
    let rendered = output::render(
        second,
        math_talk_radar_cli::cli::OutputFormat::Json,
        math_talk_radar_cli::cli::DetailLevel::Full,
    )
    .unwrap();
    std::hint::black_box(&rendered);
    let scan_ms = start.elapsed().as_secs_f64() * 1000.0;
    let peak_kb: u64 = std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let sources = radar_core::SourcesConfig::parse(&config).unwrap().sources;
    let client = radar_fetch::FetchClient::new(Default::default()).unwrap();
    let isolation = radar_fetch::fetch_all(&client, &sources[..2], None, |source| {
        if source.id == "s00" {
            Box::new(PanicAdapter)
        } else {
            radar_adapters::default_adapter(source.adapter)
        }
    })
    .await;
    assert_eq!(
        isolation[0].health.status,
        radar_core::SourceStatus::ParseError
    );
    assert_eq!(isolation[1].health.status, radar_core::SourceStatus::Ok);
    println!(
        "{}",
        serde_json::json!({"method": "mock-scan-v1", "sources": 20, "events": 400,
        "two_scans_and_output_ms": scan_ms, "peak_rss_kb": peak_kb, "release_panic_isolation": true})
    );
}
