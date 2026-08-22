//! Reproducible synthetic end-to-end baseline for release evidence (§57).
//!
//! This intentionally exercises the batch hot path rather than a single
//! parser: deterministic event construction -> conservative dedup -> state
//! write/reopen -> public JSON and JSONL streaming. It records growth at
//! 1k/5k/10k distinct events plus a 10k high-collision dedup case. The caller
//! (`cargo xtask baseline`) also builds the release CLI and passes its path via
//! `PERF_CLI_BINARY` so binary size and process startup are measured from the
//! same release build.
#![forbid(unsafe_code)]

use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use math_talk_radar_cli::cli::{DetailLevel, OutputFormat};
use math_talk_radar_cli::output::{OUTPUT_SCHEMA_VERSION, QuerySpec, ScanOutput, render_to};
use radar_core::dedup::dedup_events;
use radar_core::{
    AccessInfo, DatePrecision, DateTimeOrDate, Event, EventDate, EventId, EventStatus, EventType,
    OnlineAvailability, PublicAccess, SourceEvidence, Talk, TalkId,
};
use radar_state::Repository;
use tempfile::tempdir;
use url::Url;

const DISTINCT_CASES: &[usize] = &[1_000, 5_000, 10_000];
const HIGH_COLLISION_N: usize = 10_000;

#[derive(Debug)]
struct CaseMetrics {
    n: usize,
    dedup_ms: u128,
    state_write_ms: u128,
    reopen_ms: u128,
    json_ms: u128,
    jsonl_ms: u128,
    db_bytes: u64,
    peak_rss_kib: u64,
}

fn main() -> Result<(), Box<dyn Error>> {
    let fixed_now = Utc
        .with_ymd_and_hms(2026, 8, 23, 0, 0, 0)
        .single()
        .ok_or("invalid fixed benchmark timestamp")?;

    for &n in DISTINCT_CASES {
        let metrics = run_distinct_case(n, fixed_now)?;
        println!(
            "PERF_PIPELINE_CASE:n={};dedup_ms={};state_write_ms={};reopen_ms={};json_ms={};jsonl_ms={};db_bytes={};peak_rss_kib={}",
            metrics.n,
            metrics.dedup_ms,
            metrics.state_write_ms,
            metrics.reopen_ms,
            metrics.json_ms,
            metrics.jsonl_ms,
            metrics.db_bytes,
            metrics.peak_rss_kib,
        );
    }

    let collision_input = make_events(HIGH_COLLISION_N, true)?;
    let started = Instant::now();
    let collision_output = dedup_events(collision_input);
    let collision_ms = started.elapsed().as_millis();
    if collision_output.len() != 1 {
        return Err(format!(
            "high-collision dedup expected 1 cluster, got {}",
            collision_output.len()
        )
        .into());
    }
    println!(
        "PERF_PIPELINE_HIGH_COLLISION:n={HIGH_COLLISION_N};clusters={};dedup_ms={collision_ms};peak_rss_kib={}",
        collision_output.len(),
        peak_rss_kib().unwrap_or(0)
    );

    if let Ok(binary) = std::env::var("PERF_CLI_BINARY") {
        let binary = PathBuf::from(binary);
        let bytes = std::fs::metadata(&binary)?.len();
        let startup_ms = measure_startup(&binary)?.as_millis();
        println!("PERF_PIPELINE_BINARY_BYTES:{bytes}");
        println!("PERF_PIPELINE_STARTUP_MS:{startup_ms}");
    } else {
        println!("PERF_PIPELINE_BINARY_BYTES:unavailable");
        println!("PERF_PIPELINE_STARTUP_MS:unavailable");
    }

    println!("PERF_PIPELINE_PEAK_KB:{}", peak_rss_kib().unwrap_or(0));
    Ok(())
}

fn run_distinct_case(
    n: usize,
    fixed_now: chrono::DateTime<Utc>,
) -> Result<CaseMetrics, Box<dyn Error>> {
    let events = make_events(n, false)?;

    let started = Instant::now();
    let deduped = dedup_events(events);
    let dedup_ms = started.elapsed().as_millis();
    if deduped.len() != n {
        return Err(format!("distinct dedup expected {n} clusters, got {}", deduped.len()).into());
    }

    let dir = tempdir()?;
    let db_path = dir.path().join(format!("pipeline-{n}.redb"));
    let repo = Repository::open(&db_path)?;

    let started = Instant::now();
    let (stored, _) = repo.store_scan(&deduped, fixed_now)?;
    let state_write_ms = started.elapsed().as_millis();
    if stored.len() != n {
        return Err(format!("state write expected {n} rows, got {}", stored.len()).into());
    }
    drop(repo);
    let db_bytes = std::fs::metadata(&db_path)?.len();

    let started = Instant::now();
    let reopened = Repository::open_read_only(&db_path)?;
    let loaded = reopened.list_events()?;
    let reopen_ms = started.elapsed().as_millis();
    if loaded.len() != n {
        return Err(format!("state reopen expected {n} rows, got {}", loaded.len()).into());
    }
    drop(reopened);

    let started = Instant::now();
    render_to(
        output_for(deduped.clone(), fixed_now),
        OutputFormat::Json,
        DetailLevel::Full,
        &mut io::sink(),
    )?;
    let json_ms = started.elapsed().as_millis();

    let started = Instant::now();
    render_to(
        output_for(deduped, fixed_now),
        OutputFormat::Jsonl,
        DetailLevel::Full,
        &mut io::sink(),
    )?;
    let jsonl_ms = started.elapsed().as_millis();

    Ok(CaseMetrics {
        n,
        dedup_ms,
        state_write_ms,
        reopen_ms,
        json_ms,
        jsonl_ms,
        db_bytes,
        peak_rss_kib: peak_rss_kib().unwrap_or(0),
    })
}

fn output_for(events: Vec<Event>, generated_at: chrono::DateTime<Utc>) -> ScanOutput {
    ScanOutput {
        schema_version: OUTPUT_SCHEMA_VERSION.to_string(),
        generated_at,
        query: QuerySpec {
            mode: "both".to_string(),
            before_days: 30,
            after_days: 180,
            timezone: "UTC".to_string(),
        },
        events,
        changes: Vec::new(),
        source_health: Vec::new(),
    }
}

fn make_events(n: usize, high_collision: bool) -> Result<Vec<Event>, Box<dyn Error>> {
    let source_url = Url::parse("https://example.edu/calendar")?;
    let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 1).ok_or("invalid benchmark date")?;
    let mut out = Vec::with_capacity(n);

    for i in 0..n {
        let event_url = if high_collision {
            Url::parse("https://example.edu/events/shared")?
        } else {
            Url::parse(&format!("https://example.edu/events/{i}"))?
        };
        let source = SourceEvidence {
            source_id: "synthetic".to_string(),
            source_url: source_url.clone(),
            evidence: Some("perf_pipeline".to_string()),
            captured_at: None,
            native_id: Some(if high_collision {
                "shared".to_string()
            } else {
                format!("native-{i}")
            }),
        };
        let speaker = radar_core::PersonHit {
            canonical_name: format!("Speaker {i}"),
            matched_text: format!("Speaker {i}"),
            role: radar_core::PersonRole::Speaker,
            evidence: Some("synthetic".to_string()),
            confidence: 1.0,
            scholar_tags: Vec::new(),
        };
        let talk = Talk {
            id: TalkId(format!("talk-{i}")),
            title: format!("Synthetic talk {i}"),
            speaker: vec![speaker],
            date_time: None,
            abstract_text: Some(
                "Synthetic abstract used to exercise serialization, state persistence, and memory growth."
                    .to_string(),
            ),
            topics: Vec::new(),
            media: Vec::new(),
            source: source.clone(),
        };
        out.push(Event {
            id: EventId(format!("event-{i:05}")),
            title: if high_collision {
                "Shared synthetic event".to_string()
            } else {
                format!("Synthetic event {i}")
            },
            url: Some(event_url),
            event_type: EventType::Conference,
            status: EventStatus::Announced,
            date: EventDate {
                start: Some(DateTimeOrDate::Date(date)),
                end: None,
                timezone: None,
                original_text: "2026-09-01".to_string(),
                precision: DatePrecision::Day,
            },
            location: None,
            description: Some(
                "Synthetic event description used for release-scale performance evidence."
                    .to_string(),
            ),
            topics: Vec::new(),
            people: Vec::new(),
            talks: vec![talk],
            media: Vec::new(),
            access: AccessInfo {
                access: PublicAccess::Open,
                online: OnlineAvailability::Unknown,
            },
            sources: vec![source],
            score: 0.0,
            score_components: Default::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        });
    }
    Ok(out)
}

fn measure_startup(binary: &std::path::Path) -> Result<Duration, Box<dyn Error>> {
    let started = Instant::now();
    let status = Command::new(binary)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    let elapsed = started.elapsed();
    if !status.success() {
        return Err(format!("{} --version exited {status}", binary.display()).into());
    }
    Ok(elapsed)
}

/// Linux release CI exposes the process high-water RSS in `/proc/self/status`.
/// Other platforms return `None`; the benchmark stays runnable but records 0.
fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmHWM:")?;
        rest.split_whitespace().next()?.parse().ok()
    })
}
