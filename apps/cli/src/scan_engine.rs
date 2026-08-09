//! Scan pipeline orchestration (§12). Loads sources → filters enabled →
//! fetches via `radar-fetch` → dedups via `radar-core` → ranks → builds the
//! `ScanOutput` envelope. `--no-state` skips persistence entirely.
use std::collections::HashMap;
use std::time::Instant;

use chrono::Utc;
use radar_adapters::default_adapter;
use radar_core::dedup::dedup_events;
use radar_core::ranking::score_event;
use radar_core::{Event, config::SourceSpec, config::SourceTier};

use crate::cli::{OutputFormat, ScanArgs, ScanMode};
use crate::config_loader::load_sources;
use crate::output::{OUTPUT_SCHEMA_VERSION, QuerySpec, ScanOutput};
use crate::runtime::CliError;

pub async fn run_scan(args: ScanArgs) -> Result<ScanOutput, CliError> {
    let config = load_sources(args.sources.as_deref())?;
    let enabled: Vec<SourceSpec> = config
        .sources
        .iter()
        .filter(|s| s.enabled)
        .cloned()
        .collect();
    if enabled.is_empty() {
        return Err(CliError::zero_sources());
    }

    let client = radar_fetch::client::FetchClient::new(radar_fetch::policy::HttpPolicy::default())
        .map_err(|e| CliError::config(format!("http client build failed: {e}")))?;

    let deadline =
        Some(Instant::now() + radar_fetch::policy::HttpPolicy::default().global_scan_deadline);
    let results =
        radar_fetch::engine::fetch_all(&client, &enabled, deadline, |spec: &SourceSpec| {
            default_adapter(spec.adapter)
        })
        .await;

    let mut events: Vec<Event> = results
        .iter()
        .flat_map(|r| r.candidates.iter().map(|c| c.event.clone()))
        .collect();
    events = dedup_events(events);

    let tiers: HashMap<String, SourceTier> = config
        .sources
        .iter()
        .map(|s| (s.id.clone(), s.tier))
        .collect();
    for event in &mut events {
        let (score, components, reasons) = score_event(event, &tiers, None);
        event.score = score;
        event.score_components = components;
        event.rank_reasons = reasons;
    }

    if let Some(max) = args.max_events {
        events.truncate(max as usize);
    }

    let source_health = results.into_iter().map(|r| r.health).collect();

    let mode = match args.mode {
        ScanMode::Upcoming => "upcoming",
        ScanMode::Recordings => "recordings",
        ScanMode::Both => "both",
    };
    let output = ScanOutput {
        schema_version: OUTPUT_SCHEMA_VERSION.to_string(),
        generated_at: Utc::now(),
        query: QuerySpec {
            mode: mode.to_string(),
            before_days: args.before,
            after_days: args.after,
        },
        events,
        changes: Vec::new(),
        source_health,
    };
    let _ = OutputFormat::Json;
    Ok(output)
}
