//! Scan pipeline orchestration (§12). Loads sources → filters enabled →
//! fetches via `radar-fetch` → dedups via `radar-core` → ranks → builds the
//! `ScanOutput` envelope. `--no-state` skips persistence entirely.
use std::collections::HashMap;
use std::time::Instant;

use chrono::{NaiveDate, Utc};
use radar_adapters::default_adapter;
use radar_core::dedup::dedup_events;
use radar_core::ranking::score_event;
use radar_core::{Event, config::SourceSpec, config::SourceTier};

use crate::cli::{ScanArgs, ScanMode};
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

    let mut http_policy = radar_fetch::policy::HttpPolicy::default();
    if args.jobs > 0 {
        http_policy.global_concurrency = args.jobs as usize;
    }
    let client = radar_fetch::client::FetchClient::new(http_policy)
        .map_err(|e| CliError::config(format!("http client build failed: {e}")))?;

    let deadline = Some(Instant::now() + client.policy().global_scan_deadline);
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

    let today = match args.today.as_deref() {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
            CliError::config(format!("invalid --today {s:?}: {e}; expected YYYY-MM-DD"))
        })?,
        None => Utc::now().date_naive(),
    };
    events.retain(|e| matches_mode_and_window(e, args.mode, today, args.before, args.after));

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
    Ok(output)
}

/// §27.2 window + mode filter. An event passes when:
/// - mode is `recordings` or `both` AND the event has ≥1 media, OR
/// - mode is `upcoming` or `both` AND the event's start date falls within
///   `[today - before_days, today + after_days]`.
///
/// Events with no parseable start date are kept only in `recordings` mode
/// (if they have media) or `both` mode; in `upcoming` mode they are dropped
/// because we cannot confirm they are upcoming.
fn matches_mode_and_window(
    event: &Event,
    mode: ScanMode,
    today: NaiveDate,
    before_days: u32,
    after_days: u32,
) -> bool {
    let has_media = !event.media.is_empty();
    let want_recordings = matches!(mode, ScanMode::Recordings | ScanMode::Both);
    let want_upcoming = matches!(mode, ScanMode::Upcoming | ScanMode::Both);

    if want_recordings && has_media {
        return true;
    }

    if !want_upcoming {
        return false;
    }

    let Some(start) = event.date.start_date() else {
        return matches!(mode, ScanMode::Both);
    };
    let window_start = today - chrono::Duration::days(before_days as i64);
    let window_end = today + chrono::Duration::days(after_days as i64);
    start >= window_start && start <= window_end
}
