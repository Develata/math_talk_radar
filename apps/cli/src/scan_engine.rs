//! Scan pipeline orchestration (§12). Loads sources → filters enabled →
//! fetches via `radar-fetch` → enriches topics/people → dedups via
//! `radar-core` → ranks → builds the `ScanOutput` envelope. `--no-state`
//! skips persistence entirely.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{NaiveDate, Utc};
use radar_adapters::default_adapter;
use radar_core::dedup::dedup_events;
use radar_core::filter::{ScanMode as CoreScanMode, matches_mode_and_window};
use radar_core::ranking::{InterestWeights, score_event};
use radar_core::topics::NormalizedTopic;
use radar_core::{Event, NormalizedScholar, SourceHealth, config::SourceSpec, config::SourceTier};

use crate::cli::{ScanArgs, ScanMode};
use crate::config_loader::load_sources;
use crate::output::{OUTPUT_SCHEMA_VERSION, QuerySpec, ScanOutput};
use crate::runtime::CliError;

pub async fn run_scan(args: ScanArgs) -> Result<ScanOutput, CliError> {
    // CLI-15: validate --jobs before any I/O (config load, registry parse) so a
    // bad value fails fast instead of after disk reads.
    if args.jobs == 0 {
        return Err(CliError::usage("--jobs must be >= 1"));
    }
    // H5-2: parse --timezone before deriving `today` so the "current date"
    // used for the window reflects the user's local calendar, not UTC. Around
    // UTC midnight, `America/New_York` and `Asia/Tokyo` can differ by a full
    // calendar day. Falls back to UTC when `--timezone` is absent.
    let tz = match args.timezone.as_deref() {
        Some(tz) => {
            let parsed = radar_core::date::parse_timezone(tz).ok_or_else(|| {
                CliError::config(format!(
                    "invalid --timezone {tz:?}: unknown IANA timezone name"
                ))
            })?;
            (parsed, tz.to_string())
        }
        None => (chrono_tz::Tz::UTC, "UTC".to_string()),
    };
    // CLI-24: validate --today before any network I/O. An invalid value
    // should fail fast (exit 3) without having already burned the request
    // budget on a scan whose output would be discarded.
    let today = match args.today.as_deref() {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
            CliError::config(format!("invalid --today {s:?}: {e}; expected YYYY-MM-DD"))
        })?,
        None => Utc::now().with_timezone(&tz.0).date_naive(),
    };
    let core_mode = match args.mode {
        ScanMode::Upcoming => CoreScanMode::Upcoming,
        ScanMode::Recordings => CoreScanMode::Recordings,
        ScanMode::Both => CoreScanMode::Both,
    };
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

    // CORE-11/CORE-12: load the curated topic and scholar registries (embedded
    // defaults, §33/CFG-001) so the matchers run on every scan. Pre-normalize
    // topics once per scan so the per-event hot path skips re-normalization.
    let topics_config = radar_core::TopicsConfig::embedded()
        .map_err(|e| CliError::config(format!("embedded topics.toml: {e}")))?;
    let normalized_topics: Vec<NormalizedTopic> =
        radar_core::topics::normalize_topics(&topics_config.topics);
    // H5: --scholars override replaces the embedded default scholar registry.
    let scholars_config = match args.scholars.as_deref() {
        Some(path) => {
            let content = std::fs::read_to_string(path).map_err(|e| {
                CliError::config(format!("failed to read --scholars {path:?}: {e}"))
            })?;
            radar_core::ScholarsConfig::parse(&content).map_err(|e| {
                CliError::config(format!("failed to parse --scholars {path:?}: {e}"))
            })?
        }
        None => radar_core::ScholarsConfig::embedded()
            .map_err(|e| CliError::config(format!("embedded scholars.toml: {e}")))?,
    };
    let normalized_scholars: Vec<NormalizedScholar> =
        radar_core::normalize_scholars(&scholars_config.scholars);

    let http_policy = radar_fetch::policy::HttpPolicy {
        global_concurrency: args.jobs as usize,
        ..radar_fetch::policy::HttpPolicy::default()
    };
    let client = radar_fetch::client::FetchClient::new(http_policy)
        .map_err(|e| CliError::config(format!("http client build failed: {e}")))?;

    let deadline = Some(Instant::now() + client.policy().global_scan_deadline);
    let results =
        radar_fetch::engine::fetch_all(&client, &enabled, deadline, |spec: &SourceSpec| {
            default_adapter(spec.adapter)
        })
        .await;

    // B8-1: HTTP-005 says "zero usable sources → exit 4". The previous check
    // only fired when `enabled.is_empty()` (no sources configured at all).
    // When sources ARE configured but every one of them fails (network down,
    // all-404, all-parse-error), the scan returned exit 0 with an empty event
    // list — silently hiding a total outage. A source is "usable" if it
    // reached at least `SourceStatus::Ok` or `SourceStatus::Partial` — i.e.
    // it successfully fetched and parsed, even if it produced zero candidates
    // (a legitimately empty calendar). Only when ALL sources have a terminal
    // failure status (HttpError, ParseError, RobotsDenied, etc.) do we exit 4.
    let any_usable = results.iter().any(|r| {
        matches!(
            r.health.status,
            radar_core::SourceStatus::Ok | radar_core::SourceStatus::Partial
        )
    });
    if !any_usable {
        return Err(CliError::zero_sources());
    }

    let mut source_health: Vec<SourceHealth> = Vec::with_capacity(results.len());
    let mut events: Vec<Event> = Vec::new();
    for r in results.into_iter() {
        source_health.push(r.health);
        events.extend(r.candidates.into_iter().map(|c| c.event));
    }

    // R9-H10 (completed): global candidate cap. The per-source cap
    // (MAX_STUBS_PER_SOURCE = 2000) bounds each source, but with N enabled
    // sources the total can still reach N×2000 before any output cap fires.
    // That drives unbounded enrich/dedup/score work. Cap the total here,
    // before the expensive pipeline, so worst-case resource use is bounded
    // regardless of how many sources are enabled. The limit is generous for
    // legitimate scans (15 sources × ~600 real events ≈ 9000) while
    // preventing pathological accumulation.
    const MAX_GLOBAL_CANDIDATES: usize = 10_000;
    let capped = events.len() > MAX_GLOBAL_CANDIDATES;
    if capped {
        events.truncate(MAX_GLOBAL_CANDIDATES);
        // P0-04(b): SourceHealth.events was populated in fetch_source as
        // candidates.len() at fetch time (the pre-cap count). When the global
        // cap drops later-sorted sources' events entirely, the persisted
        // health reported the original count while state held the capped set
        // — an inconsistency that made source_health misleading. Recount
        // per-source survivors so health reflects the events actually
        // carried forward into the pipeline and persisted state.
        //
        // R3-P0-02: a source whose events were dropped by the global cap is
        // no longer complete — mark it Partial so the ADR-0012 prune guard
        // in store_scan_bundle suppresses cancellation of the truncated
        // events. Without this, the next scan would tombstone and delete
        // live events that were merely cut by the cap.
        let mut survivors: HashMap<String, u32> = HashMap::new();
        for ev in &events {
            for s in &ev.sources {
                *survivors.entry(s.source_id.clone()).or_default() += 1;
            }
        }
        for h in &mut source_health {
            let survivor_count = survivors.get(&h.source).copied().unwrap_or(0);
            if h.events > survivor_count {
                h.status = radar_core::SourceStatus::Partial;
            }
            h.events = survivor_count;
        }
    }

    // CORE-11/CORE-12: enrich each event before the first scoring pass so the
    // topic (30pt) and people (10pt) components reflect real matches and
    // influence dedup primary selection. Topic matching populates event.topics
    // from the title + description. Scholar enrichment back-fills scholar_tags
    // on adapter-found people and adds title-mentioned scholars not already
    // present, so the people component can recognize important laureates.
    for event in &mut events {
        radar_core::enrich_event_topics(event, &normalized_topics);
        radar_core::enrich_event_scholars(event, &normalized_scholars);
    }

    let tiers: HashMap<String, SourceTier> = config
        .sources
        .iter()
        .map(|s| (s.id.clone(), s.tier))
        .collect();
    apply_scores(&mut events, &tiers, interests_ref);

    events = dedup_events(events);

    apply_scores(&mut events, &tiers, interests_ref);

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

    // ADR-0011 §6: stamp recorded_at before store_scan_bundle persists
    // per-scan history.
    for h in &mut source_health {
        h.recorded_at = Some(now);
    }

    let (mut events, changes) = if args.no_state {
        (events, Vec::new())
    } else {
        match open_state_repo(args.state.as_deref()) {
            Ok(repo) => match repo.store_scan_bundle(&events, &source_health, now) {
                Ok((stored, changes)) => (stored, changes),
                // CLI-21: the DB opened but the write failed — that is a
                // state-fatal condition (§32 exit 5), not a best-effort
                // degradation. The user did not ask for --no-state; silently
                // swallowing a write failure would lose change-detection
                // signals and corrupt the first_seen timeline.
                Err(e) => return Err(CliError::state(format!("state store_scan failed: {e}"))),
            },
            // H02: when the user explicitly passed --state <path>, an open
            // failure (permission denied, missing parent dir, schema mismatch)
            // is a state-fatal error (§32 exit 5), not a silent degrade. The
            // explicit path signals that state is required for this run.
            // Only the default-derived path (no --state) degrades to no-state
            // with a warning — there the user did not express a dependency.
            Err(e) if args.state.is_some() => {
                return Err(CliError::state(format!(
                    "could not open state repository at {}: {e}",
                    args.state.as_ref().unwrap().display()
                )));
            }
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
    events.retain(|e| matches_mode_and_window(e, core_mode, today, args.before, args.after));

    if let Some(max) = args.max_events {
        events.truncate(max as usize);
    }

    // H5-3: --max-talks caps the total number of talks across all emitted
    // events (§27.2, default 300). Unlike --max-events, this preserves event
    // envelopes but truncates their talks to fit the remaining budget. An
    // event whose talks are all consumed by the cap still appears with an
    // empty talks list; subsequent events are dropped entirely once the
    // budget is exhausted.
    let max_talks = args.max_talks.unwrap_or(300) as usize;
    let mut remaining = max_talks;
    let mut kept = Vec::new();
    for mut e in events {
        if remaining == 0 && max_talks > 0 {
            break;
        }
        if e.talks.len() > remaining {
            e.talks.truncate(remaining);
        }
        remaining = remaining.saturating_sub(e.talks.len());
        kept.push(e);
    }
    events = kept;

    let output = ScanOutput {
        schema_version: OUTPUT_SCHEMA_VERSION.to_string(),
        generated_at: now,
        query: QuerySpec {
            mode: core_mode.as_str().to_string(),
            before_days: args.before,
            after_days: args.after,
            timezone: tz.1,
        },
        events,
        changes,
        source_health,
    };
    Ok(output)
}

fn apply_scores(
    events: &mut [Event],
    tiers: &HashMap<String, SourceTier>,
    interests: Option<&InterestWeights>,
) {
    for event in events {
        let (score, components, reasons) = score_event(event, tiers, interests);
        event.score = score;
        event.score_components = components;
        event.rank_reasons = reasons;
    }
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
