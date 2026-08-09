use std::time::SystemTime;

use radar_adapters::rss::RssAdapter;
use radar_core::config::{AdapterKind, SourceKind, SourceSpec, SourceTier};
use radar_core::{FetchedDocument, SourceAdapter};
use url::Url;

fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixtures: &[(&str, &str, &str)] = &[
        (
            "clay",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/sites/clay-list.xml"
            ),
            "https://www.claymath.org/events/feed/",
        ),
        (
            "ihes",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/sites/ihes-list.xml"
            ),
            "https://www.ihes.fr/en/category/event/feed/",
        ),
    ];
    let _ = manifest_dir;

    let adapter = RssAdapter;
    let mut total_events = 0usize;

    for _ in 0..200 {
        for (id, path, url) in fixtures {
            let body = std::fs::read_to_string(path).expect("fixture exists");
            let parsed_url = Url::parse(url).unwrap();
            let doc = FetchedDocument {
                url: parsed_url.clone(),
                final_url: parsed_url,
                status: 200,
                content_type: Some("application/rss+xml".into()),
                body: body.into_bytes(),
                fetched_at: SystemTime::now().into(),
            };
            let source = SourceSpec {
                id: (*id).into(),
                name: (*id).into(),
                tier: SourceTier::default(),
                kind: SourceKind::default(),
                adapter: AdapterKind::Rss,
                entrypoint: Some(Url::parse(url).unwrap()),
                allowed_hosts: Vec::new(),
                max_depth: 2,
                request_budget: 20,
                media_strategy: None,
                dynamic: false,
                enabled: true,
                fixture: None,
                selectors: None,
            };
            let stubs = adapter.discover(&doc, &source).expect("discover ok");
            for stub in stubs {
                let detail_doc = FetchedDocument {
                    url: stub.url.clone(),
                    final_url: stub.url.clone(),
                    status: 200,
                    content_type: Some("text/html".into()),
                    body: b"<html><body><p>detail</p></body></html>".to_vec(),
                    fetched_at: SystemTime::now().into(),
                };
                let _ = adapter
                    .enrich(stub, &[detail_doc], &source)
                    .expect("enrich ok");
                total_events += 1;
            }
        }
    }

    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    let vmhwm_kb: u64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .expect("VmHWM line present");

    println!("PERF_RSS_EVENTS: {total_events}");
    println!("PERF_RSS_PEAK_KB: {vmhwm_kb}");
}
