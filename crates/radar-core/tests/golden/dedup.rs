//! Dedup golden tests — parses `dedup.toml` and evaluates each pair against
//! `radar_core::dedup::duplicate_signal`. Also covers REL-003 (stable
//! deterministic IDs) via `deterministic_id`.

use chrono::NaiveDate;
use radar_core::{
    AccessInfo, DatePrecision, DateTimeOrDate, Event, EventDate, EventId, EventStatus, EventType,
    Location, OnlineAvailability, PersonHit, PersonRole, PublicAccess, ScoreComponents,
    SourceEvidence, dedup, deterministic_id,
};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
pub(crate) struct Pairs {
    pairs: Vec<DedupPair>,
}

#[derive(Deserialize)]
pub(crate) struct DedupPair {
    id: String,
    should_merge: bool,
    #[serde(default)]
    expected_signal: Option<String>,
    a: EventSpec,
    b: EventSpec,
}

#[derive(Deserialize)]
pub(crate) struct EventSpec {
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    organizer: Option<String>,
    #[serde(default)]
    location: Option<String>,
    source_id: String,
    source_url: String,
    #[serde(default)]
    native_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct DedupStats {
    pub total: usize,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub true_negatives: usize,
    pub failures: Vec<String>,
}

fn build_event(spec: &EventSpec, id_suffix: &str) -> Event {
    let start = spec.start_date.as_deref().and_then(parse_date);
    let mut people = Vec::new();
    if let Some(org) = &spec.organizer {
        people.push(PersonHit {
            canonical_name: org.clone(),
            matched_text: org.clone(),
            role: PersonRole::Organizer,
            evidence: None,
            confidence: 1.0,
            scholar_tags: Vec::new(),
        });
    }
    let location = spec.location.as_ref().map(|name| Location {
        name: name.clone(),
        city: None,
        country: None,
        venue: None,
    });
    let sources = vec![SourceEvidence {
        source_id: spec.source_id.clone(),
        source_url: Url::parse(&spec.source_url).expect("valid url in fixture"),
        evidence: None,
        captured_at: None,
        native_id: spec.native_id.clone(),
    }];
    Event {
        id: EventId(format!("dedup-{id_suffix}")),
        title: spec.title.clone(),
        url: spec
            .url
            .as_deref()
            .map(|u| Url::parse(u).expect("valid url")),
        event_type: EventType::Conference,
        status: EventStatus::Unknown,
        date: EventDate {
            start: start.map(DateTimeOrDate::Date),
            end: None,
            timezone: None,
            original_text: String::new(),
            precision: start.map_or(DatePrecision::Unknown, |_| DatePrecision::Day),
        },
        location,
        description: None,
        topics: Vec::new(),
        people,
        talks: Vec::new(),
        media: Vec::new(),
        access: AccessInfo {
            access: PublicAccess::Unknown,
            online: OnlineAvailability::Unknown,
        },
        sources,
        score: 0.0,
        score_components: ScoreComponents::default(),
        rank_reasons: Vec::new(),
        first_seen_at: None,
        last_seen_at: None,
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn signal_name(signal: dedup::DedupSignal) -> &'static str {
    match signal {
        dedup::DedupSignal::CanonicalUrl => "CanonicalUrl",
        dedup::DedupSignal::SourceCanonicalId => "SourceCanonicalId",
        dedup::DedupSignal::TitleDateOrganizer => "TitleDateOrganizer",
        dedup::DedupSignal::TitleDateLocation => "TitleDateLocation",
    }
}

pub fn run(data: &str) -> DedupStats {
    let parsed: Pairs = toml::from_str(data).expect("dedup.toml parses");
    let total = parsed.pairs.len();
    let mut stats = DedupStats {
        total,
        ..Default::default()
    };

    for pair in &parsed.pairs {
        let a = build_event(&pair.a, "a");
        let b = build_event(&pair.b, "b");
        let signal = dedup::duplicate_signal(&a, &b);
        let detected = signal.is_some();
        match (pair.should_merge, detected) {
            (true, true) => {
                stats.true_positives += 1;
                if let Some(expected) = &pair.expected_signal {
                    let actual = signal_name(signal.expect("checked is_some above"));
                    if expected != actual {
                        stats.failures.push(format!(
                            "{}: expected signal {} but got {}",
                            pair.id, expected, actual
                        ));
                    }
                }
            }
            (true, false) => {
                stats.false_negatives += 1;
                stats
                    .failures
                    .push(format!("{}: expected merge but no signal matched", pair.id));
            }
            (false, true) => {
                stats.false_positives += 1;
                stats.failures.push(format!(
                    "{}: expected no merge but a signal matched",
                    pair.id
                ));
            }
            (false, false) => stats.true_negatives += 1,
        }
    }

    stats
}

/// REL-003: `deterministic_id` produces stable IDs across repeated calls with
/// identical inputs, and distinct IDs for distinct inputs.
pub fn run_rel003() {
    let parts_a = ["Algebraic Geometry Conference", "mit.edu", "2026-08-09"];
    let parts_b = ["Number Theory Workshop", "msri.org", "2026-09-15"];

    let id_a1 = deterministic_id(&parts_a);
    let id_a2 = deterministic_id(&parts_a);
    let id_b1 = deterministic_id(&parts_b);

    assert_eq!(
        id_a1, id_a2,
        "REL-003: same inputs must produce identical IDs across calls"
    );
    assert_ne!(
        id_a1, id_b1,
        "REL-003: distinct inputs must produce distinct IDs"
    );
    assert!(
        id_a1.starts_with("blake3:"),
        "REL-003: ID must be BLAKE3-prefixed, got {id_a1}"
    );
}
