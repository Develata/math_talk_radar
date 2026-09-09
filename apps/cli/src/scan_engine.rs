//! Composition root: load → fetch → enrich → dedup → persist → rank/filter → output.
//! Ranking runs after state reconciliation so it is purely presentation policy.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{DateTime, Utc};
use radar_core::dedup::{dedup_events, dedup_events_with_aliases};
use radar_core::filter::matches_mode_and_window;
use radar_core::ranking::score_event;
use radar_core::{Event, EventId, SourceHealth, SourceStatus};
use radar_state::ChangeRecord;

use crate::cli::ScanArgs;
use crate::output::{OUTPUT_SCHEMA_VERSION, QuerySpec, ScanOutput};
use crate::runtime::CliError;

mod context;
mod enrichment;
use context::ScanContext;

pub async fn run_scan(args: ScanArgs) -> Result<ScanOutput, CliError> {
    let context = context::load(&args)?;
    let policy = radar_fetch::HttpPolicy {
        global_concurrency: args.jobs as usize,
        ..Default::default()
    };
    let client = radar_fetch::FetchClient::new(policy)
        .map_err(|e| CliError::config(format!("http client build failed: {e}")))?;
    let deadline = Some(Instant::now() + policy.global_scan_deadline);
    let results = radar_fetch::fetch_all(&client, &context.enabled, deadline, |spec| {
        radar_adapters::default_adapter(spec.adapter)
    })
    .await;
    if !results
        .iter()
        .any(|r| matches!(r.health.status, SourceStatus::Ok | SourceStatus::Partial))
    {
        return Err(CliError::zero_sources());
    }
    let mut events = Vec::new();
    let mut source_health = Vec::with_capacity(results.len());
    for result in results {
        events.extend(
            result
                .candidates
                .into_iter()
                .map(|candidate| candidate.event),
        );
        source_health.push(result.health);
    }
    for event in &mut events {
        enrichment::enrich_event_topics(event, &context.topics);
        enrichment::enrich_event_scholars(event, &context.scholars);
    }
    let (events, aliases) = if args.no_state {
        (dedup_events(events), HashMap::new())
    } else {
        dedup_events_with_aliases(events)
    };
    let now = Utc::now();
    let (mut events, changes) = persist_scan(events, &args, now, &source_health, &aliases)?;
    for event in &mut events {
        (event.score, event.score_components, event.rank_reasons) =
            score_event(event, &context.tiers, context.interests.as_ref());
    }
    events.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });
    filter_output(&mut events, &args, &context);
    Ok(ScanOutput {
        schema_version: OUTPUT_SCHEMA_VERSION.to_owned(),
        generated_at: now,
        query: QuerySpec {
            mode: context.mode.as_str().to_owned(),
            before_days: args.before,
            after_days: args.after,
            timezone: context.timezone,
        },
        events,
        changes,
        source_health,
    })
}

fn persist_scan(
    events: Vec<Event>,
    args: &ScanArgs,
    now: DateTime<Utc>,
    health: &[SourceHealth],
    aliases: &HashMap<EventId, EventId>,
) -> Result<(Vec<Event>, Vec<ChangeRecord>), CliError> {
    if args.no_state {
        return Ok((events, Vec::new()));
    }
    let repo = match open_state_repo(args.state.as_deref()) {
        Ok(repo) => repo,
        Err(error) => {
            if let Some(path) = &args.state {
                return Err(CliError::state(format!(
                    "could not open state repository at {}: {error}",
                    path.display()
                )));
            }
            eprintln!(
                "warning: could not open state repository: {error}; continuing without state"
            );
            return Ok((events, Vec::new()));
        }
    };
    let authoritative: HashSet<String> = health
        .iter()
        .filter(|h| h.status == SourceStatus::Ok)
        .map(|h| h.source.clone())
        .collect();
    // Full observed corpus before all output filters/caps. Ranking fields are
    // still adapter defaults; the persisted format remains backward compatible.
    repo.store_scan_with_authority(events, now, &authoritative, aliases)
        .map_err(|e| CliError::state(format!("state store_scan failed: {e}")))
}

fn filter_output(events: &mut Vec<Event>, args: &ScanArgs, context: &ScanContext) {
    events.retain(|e| {
        matches_mode_and_window(e, context.mode, context.today, args.before, args.after)
    });
    if let Some(max) = args.max_events {
        events.truncate(max as usize);
    }
    let max_talks = args.max_talks.unwrap_or(300) as usize;
    let mut remaining = max_talks;
    let mut kept = 0;
    for event in events.iter_mut() {
        if remaining == 0 && max_talks > 0 {
            break;
        }
        event.talks.truncate(remaining);
        remaining = remaining.saturating_sub(event.talks.len());
        kept += 1;
    }
    events.truncate(kept);
}

fn default_state_db_path() -> Option<PathBuf> {
    // CLI-13: delegate to the shared XDG resolver instead of duplicating the
    // XDG_DATA_HOME / HOME fallback here. data_dir() never returns None (it
    // falls back to a relative path), so gate on the env vars to preserve the
    // Option contract: the caller reports a clear error when both are unset.
    if std::env::var_os("XDG_DATA_HOME").is_none() && std::env::var_os("HOME").is_none() {
        return None;
    }
    Some(crate::lifecycle::paths::data_dir().join("state.redb"))
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
