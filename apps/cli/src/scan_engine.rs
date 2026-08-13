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
use radar_core::normalize::normalize_name;
use radar_core::people::{MatchContext, ScholarRecord};
use radar_core::ranking::{InterestWeights, score_event};
use radar_core::topics::{NormalizedTopic, match_topics_normalized};
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

    // CORE-11/CORE-12: load the curated topic and scholar registries (embedded
    // defaults, §33/CFG-001) so the matchers run on every scan. Pre-normalize
    // topics once per scan so the per-event hot path skips re-normalization.
    let topics_config = radar_core::TopicsConfig::embedded();
    let normalized_topics: Vec<NormalizedTopic> =
        radar_core::topics::normalize_topics(&topics_config.topics);
    let scholars_config = radar_core::ScholarsConfig::embedded();
    let scholars: &[ScholarRecord] = &scholars_config.scholars;

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

    // CORE-11/CORE-12: enrich each event before the first scoring pass so the
    // topic (30pt) and people (10pt) components reflect real matches and
    // influence dedup primary selection. Topic matching populates event.topics
    // from the title + description. Scholar enrichment back-fills scholar_tags
    // on adapter-found people and adds title-mentioned scholars not already
    // present, so the people component can recognize important laureates.
    for event in &mut events {
        enrich_event_topics(event, &normalized_topics);
        enrich_event_scholars(event, scholars);
    }

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

/// CORE-11: match the event's title and description against the curated topic
/// registry and populate `event.topics`. Adapters set `topics: Vec::new()`;
/// without this step the 30-point topic component is always zero.
///
/// The title is matched first (canonical names are more likely there), then
/// the description; matches are deduplicated by `topic_id`, keeping the
/// highest-confidence hit so a canonical title match beats an alias match from
/// the description.
fn enrich_event_topics(event: &mut Event, topics: &[NormalizedTopic]) {
    if topics.is_empty() {
        return;
    }
    let mut matches = match_topics_normalized(&event.title, topics);
    if let Some(desc) = event.description.as_ref()
        && !desc.is_empty()
    {
        let desc_matches = match_topics_normalized(desc, topics);
        for m in desc_matches {
            if let Some(existing) = matches.iter_mut().find(|e| e.topic_id == m.topic_id) {
                if m.confidence > existing.confidence {
                    *existing = m;
                }
            } else {
                matches.push(m);
            }
        }
    }
    if !matches.is_empty() || !event.topics.is_empty() {
        event.topics = matches;
    }
}

/// CORE-12: enrich the event's people list with scholar tags from the curated
/// registry and add title-mentioned scholars that adapters did not surface.
/// Adapters construct `PersonHit`s from parsed speaker fields but set
/// `scholar_tags: Vec::new()`; without this step the people component can
/// never exceed the 3-point baseline for a non-TitleMention role, so
/// important laureates (Fields/Abel/Wolf/Crafoord) are never recognized.
///
/// Two passes:
/// 1. For each adapter-found person, look up the scholar by normalized
///    canonical name. On a hit, back-fill `scholar_tags` and correct the
///    `canonical_name` to the registry's canonical form (adapters may use a
///    variant surface form).
/// 2. Run `match_scholars` on the title under `TitleText` context to find
///    important scholars mentioned only in the title. Add any not already
///    present (by normalized canonical name) as `TitleMention` so the people
///    component can still recognize them for ranking.
fn enrich_event_scholars(event: &mut Event, scholars: &[ScholarRecord]) {
    if scholars.is_empty() {
        return;
    }

    // Pass 1: back-fill scholar_tags on adapter-found people.
    for person in &mut event.people {
        if !person.scholar_tags.is_empty() {
            continue;
        }
        let norm_name = normalize_name(&person.canonical_name);
        if let Some(scholar) = scholars
            .iter()
            .find(|s| normalize_name(&s.canonical_name) == norm_name)
        {
            person.scholar_tags = scholar.tags.clone();
            person.canonical_name = scholar.canonical_name.clone();
        }
    }

    // Pass 2: add title-mentioned scholars not already present.
    let title_hits =
        radar_core::people::match_scholars(&event.title, scholars, MatchContext::TitleText);
    for hit in title_hits {
        let norm_canonical = normalize_name(&hit.canonical_name);
        let already_present = event
            .people
            .iter()
            .any(|p| normalize_name(&p.canonical_name) == norm_canonical);
        if !already_present {
            event.people.push(hit);
        }
    }
}
