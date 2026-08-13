//! Scan pipeline orchestration (§12). Loads sources → filters enabled →
//! fetches via `radar-fetch` → dedups via `radar-core` → ranks → builds the
//! `ScanOutput` envelope. `--no-state` skips persistence entirely.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{NaiveDate, Utc};
use radar_adapters::default_adapter;
use radar_core::dedup::dedup_events;
use radar_core::filter::{ScanMode as CoreScanMode, matches_mode_and_window};
use radar_core::ranking::{InterestWeights, score_event};
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

    let interests = match args.interests.as_deref() {
        Some(path) => {
            let content = std::fs::read_to_string(path).map_err(|e| {
                CliError::config(format!("failed to read --interests {path:?}: {e}"))
            })?;
            Some(InterestWeights::parse(&content).map_err(|e| {
                CliError::config(format!("failed to parse --interests {path:?}: {e}"))
            })?)
        }
        None => None,
    };
    let interests_ref = interests.as_ref();

    let mut http_policy = radar_fetch::policy::HttpPolicy::default();
    if args.jobs == 0 {
        return Err(CliError::usage("--jobs must be >= 1"));
    }
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

    let tiers: HashMap<String, SourceTier> = config
        .sources
        .iter()
        .map(|s| (s.id.clone(), s.tier))
        .collect();
    for event in &mut events {
        let (score, components, reasons) = score_event(event, &tiers, interests_ref);
        event.score = score;
        event.score_components = components;
        event.rank_reasons = reasons;
    }

    events = dedup_events(events);

    for event in &mut events {
        let (score, components, reasons) = score_event(event, &tiers, interests_ref);
        event.score = score;
        event.score_components = components;
        event.rank_reasons = reasons;
    }

    events.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });

    // Store the FULL scan result (pre-filter, pre-truncate) so change detection
    // compares against every live event. Filtering by mode/window or capping
    // with --max-events affects the OUTPUT only — an event filtered out of a
    // given query must NOT be recorded as `EventCancelled` in persisted state
    // (CLI-10: regression from ST-1 wiring that ran store_scan after the cap).
    let now = Utc::now();
    let (mut events, changes) = if args.no_state {
        (events, Vec::new())
    } else {
        match open_state_repo(args.state.as_deref()) {
            Ok(repo) => match repo.store_scan(&events, now) {
                Ok((stored, changes)) => (stored, changes),
                Err(e) => {
                    eprintln!("warning: state store_scan failed: {e}; continuing without state");
                    (events, Vec::new())
                }
            },
            Err(e) => {
                eprintln!(
                    "warning: could not open state repository: {e}; continuing without state"
                );
                (events, Vec::new())
            }
        }
    };

    // Apply the mode/window filter and --max-events cap to the OUTPUT only.
    // The persisted state already reflects the full scan above.
    let today = match args.today.as_deref() {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
            CliError::config(format!("invalid --today {s:?}: {e}; expected YYYY-MM-DD"))
        })?,
        None => Utc::now().date_naive(),
    };
    let core_mode = match args.mode {
        ScanMode::Upcoming => CoreScanMode::Upcoming,
        ScanMode::Recordings => CoreScanMode::Recordings,
        ScanMode::Both => CoreScanMode::Both,
    };
    events.retain(|e| matches_mode_and_window(e, core_mode, today, args.before, args.after));

    if let Some(max) = args.max_events {
        events.truncate(max as usize);
    }

    let source_health = results.into_iter().map(|r| r.health).collect();

    let output = ScanOutput {
        schema_version: OUTPUT_SCHEMA_VERSION.to_string(),
        generated_at: now,
        query: QuerySpec {
            mode: core_mode.as_str().to_string(),
            before_days: args.before,
            after_days: args.after,
        },
        events,
        changes,
        source_health,
    };
    Ok(output)
}

fn default_state_db_path() -> Option<PathBuf> {
    let dir = if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        let mut p = PathBuf::from(xdg);
        p.push("math_talk_radar");
        p
    } else {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push(".local");
        p.push("share");
        p.push("math_talk_radar");
        p
    };
    Some(dir.join("state.redb"))
}

fn open_state_repo(override_path: Option<&Path>) -> Result<radar_state::Repository, String> {
    let path = match override_path {
        Some(p) => p.to_path_buf(),
        None => default_state_db_path()
            .ok_or_else(|| "no state directory (XDG_DATA_HOME and HOME both unset)".to_string())?,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create state dir {parent:?}: {e}"))?;
    }
    radar_state::Repository::open(&path).map_err(|e| format!("open state db {path:?}: {e}"))
}
