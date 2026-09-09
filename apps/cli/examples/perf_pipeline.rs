//! Offline scaling probe. Setup is excluded from dedup timings; each workload
//! uses three independent runs and reports the median. No live HTTP requests.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

use radar_core::dedup::dedup_events;
use radar_core::ranking::score_event;
use radar_core::{Event, SourceAdapter};
use serde_json::json;

fn template() -> Event {
    let source = radar_core::SourcesConfig::embedded()
        .expect("embedded sources")
        .sources
        .into_iter()
        .find(|s| s.id == "clay")
        .expect("clay fixture source");
    let url = source.entrypoint.clone().expect("fixture URL");
    let document = radar_core::FetchedDocument {
        url: url.clone(),
        final_url: url,
        status: 200,
        content_type: Some("application/rss+xml".into()),
        body: include_bytes!("../../../crates/radar-adapters/tests/fixtures/sites/clay-list.xml")
            .to_vec(),
        fetched_at: chrono::DateTime::UNIX_EPOCH,
    };
    let adapter = radar_adapters::rss::RssAdapter;
    let stub = adapter
        .discover(&document, &source)
        .expect("fixture discovery")
        .remove(0);
    adapter
        .enrich(stub, &[], &source)
        .expect("fixture enrichment")
        .event
}

fn corpus(template: &Event, n: usize, collision: bool) -> Vec<Event> {
    (0..n)
        .map(|i| {
            let mut event = template.clone();
            let key = if collision { i % 10 } else { i };
            event.title = format!("Mathematics conference {key}");
            event.url = Some(format!("https://example.org/events/{key}").parse().unwrap());
            event.id = radar_core::event_id(&event.title, event.url.as_ref().unwrap().as_str());
            // Fixed-size provenance per cluster: collisions exercise merge overhead
            // without conflating it with a growing evidence collection.
            for source in &mut event.sources {
                source.source_url = event.url.clone().unwrap();
                source.native_id = Some(key.to_string());
            }
            event
        })
        .collect()
}

fn main() {
    let template = template();
    let mut dedup_ms = serde_json::Map::new();
    for (name, n, collision) in [
        ("1000", 1000, false),
        ("5000", 5000, false),
        ("10000", 10000, false),
        ("collision_10000", 10000, true),
    ] {
        let mut times = Vec::new();
        for _ in 0..3 {
            let events = corpus(&template, n, collision);
            let start = Instant::now();
            let result = dedup_events(black_box(events));
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(result.len(), if collision { 10 } else { n });
            black_box(result);
        }
        times.sort_by(f64::total_cmp);
        dedup_ms.insert(name.into(), json!(times[1]));
    }

    let dir = tempfile::tempdir().expect("temporary state");
    let repo = radar_state::Repository::open(&dir.path().join("state.redb")).expect("state open");
    let mut events = corpus(&template, 10_000, false);
    let now = chrono::DateTime::UNIX_EPOCH;
    let tiers = HashMap::new();
    let start = Instant::now();
    events = dedup_events(events);
    for event in &mut events {
        let (score, components, reasons) = score_event(event, &tiers, None);
        event.score = score;
        event.score_components = components;
        event.rank_reasons = reasons;
    }
    events.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });
    let state_start = Instant::now();
    let (events, _) = repo
        .store_scan_owned(events, now)
        .expect("first state scan");
    let (events, changes) = repo
        .store_scan_owned(events, now)
        .expect("unchanged state scan");
    assert!(changes.is_empty());
    let state_ms = state_start.elapsed().as_secs_f64() * 1000.0;
    let json_start = Instant::now();
    let bytes = serde_json::to_vec(&events).expect("JSON");
    black_box(&bytes);
    let json_ms = json_start.elapsed().as_secs_f64() * 1000.0;
    let jsonl_start = Instant::now();
    let mut jsonl = Vec::new();
    for event in &events {
        serde_json::to_writer(&mut jsonl, event).expect("JSONL");
        jsonl.push(b'\n');
    }
    black_box(&jsonl);
    let jsonl_ms = jsonl_start.elapsed().as_secs_f64() * 1000.0;
    let pipeline_ms = start.elapsed().as_secs_f64() * 1000.0;
    let peak_kb: u64 = std::fs::read_to_string("/proc/self/status")
        .expect("Linux RSS")
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .expect("VmHWM");
    println!(
        "{}",
        json!({"method": "offline-v1", "dedup_ms": dedup_ms, "pipeline_10000_ms": pipeline_ms, "state_two_scans_ms": state_ms, "json_ms": json_ms, "jsonl_ms": jsonl_ms, "json_bytes": bytes.len(), "peak_rss_kb": peak_kb})
    );
}
