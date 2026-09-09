//! Validate query arguments and load configuration before network/state I/O.
use crate::cli::{ScanArgs, ScanMode};
use crate::config_loader::load_sources;
use crate::runtime::CliError;
use chrono::{NaiveDate, Utc};
use radar_core::filter::ScanMode as CoreScanMode;
use radar_core::people::ScholarRecord;
use radar_core::ranking::InterestWeights;
use radar_core::topics::NormalizedTopic;
use radar_core::{SourceSpec, SourceTier};
use std::collections::HashMap;

pub(super) struct ScanContext {
    pub enabled: Vec<SourceSpec>,
    pub tiers: HashMap<String, SourceTier>,
    pub interests: Option<InterestWeights>,
    pub topics: Vec<NormalizedTopic>,
    pub scholars: Vec<ScholarRecord>,
    pub today: NaiveDate,
    pub timezone: String,
    pub mode: CoreScanMode,
}

pub(super) fn load(args: &ScanArgs) -> Result<ScanContext, CliError> {
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

    let tiers = config.sources.into_iter().map(|s| (s.id, s.tier)).collect();
    Ok(ScanContext {
        enabled,
        tiers,
        interests,
        topics: normalized_topics,
        scholars: scholars_config.scholars,
        today,
        timezone: tz.1,
        mode: core_mode,
    })
}
