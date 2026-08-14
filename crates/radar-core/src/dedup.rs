//! Cross-source deduplication (§25, §47).
//!
//! Conservative deterministic dedup. Signals tried in priority order:
//! canonical URL → source-declared canonical ID → normalized
//! title+date+organizer → normalized title+date+location. We prefer keeping a
//! suspected duplicate over merging two distinct events: a wrong merge is a
//! release blocker (§47). Fuzzy/semantic dedup is deferred.
//!
//! Determinism: identical inputs produce identical merge decisions. No clocks,
//! no randomness, no order-dependent ties (we sort before merging).
use std::collections::HashSet;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::model::{Event, Location, MediaResource, SourceEvidence, Talk};
use crate::normalize::{canonicalize_url, normalize_name};
use crate::people::{PersonHit, PersonRole};
use crate::topics::TopicMatch;

/// Identity signal used to decide whether two events are the same (§25).
/// Listed weakest-to-strongest by the algorithm's preference; the actual
/// matching order is strongest-to-weakest (CanonicalUrl first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DedupSignal {
    /// Identical canonical URL.
    CanonicalUrl,
    /// Source declares the same canonical ID (e.g. Indico event id).
    SourceCanonicalId,
    /// Normalized title + start date + organizer match.
    TitleDateOrganizer,
    /// Normalized title + start date + location match.
    TitleDateLocation,
}

impl DedupSignal {
    /// Strongest-to-weakest matching order per §25.
    pub const PRIORITY: [DedupSignal; 4] = [
        DedupSignal::CanonicalUrl,
        DedupSignal::SourceCanonicalId,
        DedupSignal::TitleDateOrganizer,
        DedupSignal::TitleDateLocation,
    ];
}

/// Earliest calendar start date among an event's sources, or `None` if the
/// event has no parseable start.
fn start_date(event: &Event) -> Option<NaiveDate> {
    event.date.start_date()
}

/// First organizer name (normalized) found among the event's people, else the
/// registrable domain of the first source (the "organizer/domain" fallback of
/// §24). Returns `None` only if there are no sources and no organizer person.
fn organizer_key(event: &Event) -> Option<String> {
    for p in &event.people {
        if p.role == PersonRole::Organizer {
            return Some(normalize_name(&p.canonical_name));
        }
    }
    // Fall back to the eTLD+1 of the first source's URL (the domain acts as
    // organizer per §24's "canonical organizer/domain" identity field).
    event
        .sources
        .first()
        .and_then(|s| domain_key(&s.source_url))
}

/// Second-level domains that act as effective TLDs (e.g. `ac.uk`, `co.jp`).
/// When the host ends with one of these, the registrable domain is the last
/// three labels (e.g. `maths.ox.ac.uk` → `ox.ac.uk`); otherwise the last two.
const MULTI_PART_TLDS: &[&str] = &[
    "ac.uk", "co.uk", "gov.uk", "org.uk", "me.uk", "edu.au", "com.au", "org.au", "net.au",
    "gov.au", "ac.jp", "co.jp", "go.jp", "or.jp", "ne.jp", "ac.kr", "co.kr", "go.kr", "or.kr",
    "edu.cn", "ac.cn", "gov.cn", "com.cn", "org.cn", "edu.tw", "ac.tw", "gov.tw", "ac.nz", "co.nz",
    "govt.nz", "edu.sg", "com.sg", "org.sg", "gov.sg", "ac.il", "co.il", "com.br", "org.br",
    "edu.br", "gov.br", "com.hk", "org.hk", "edu.hk", "gov.hk", "com.mx", "org.mx", "edu.mx",
];

/// Extract the registrable domain (eTLD+1 approximation): for multi-part TLD
/// suffixes (e.g. `.ac.uk`), return the last three labels; otherwise the last
/// two. This is a coarse, suffix-list-free approximation suitable only for
/// dedup grouping, not security.
fn domain_key(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    for suffix in MULTI_PART_TLDS {
        if (host.ends_with(suffix)
            && host.len() > suffix.len()
            && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
            || host == *suffix)
            && labels.len() >= 3
        {
            return Some(labels[labels.len() - 3..].join("."));
        }
    }
    if labels.len() <= 2 {
        Some(host)
    } else {
        Some(labels[labels.len() - 2..].join("."))
    }
}

/// Normalized location key: city (if present) else venue name (if present) else
/// the full location name, all normalized. Returns `None` if no location.
fn location_key(loc: &Location) -> Option<String> {
    let raw = loc
        .city
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| loc.venue.as_deref().filter(|s| !s.trim().is_empty()))
        .unwrap_or(&loc.name);
    if raw.trim().is_empty() {
        None
    } else {
        Some(normalize_name(raw))
    }
}

/// Precomputed dedup identity fields for one event. Computed once per event
/// and reused across the O(n²) cluster scan in `dedup_events`, instead of
/// re-normalizing title/organizer/location/URL on every pairwise comparison.
/// Must be recomputed for a cluster representative after a merge, since the
/// merge can union in new sources, location, or people.
struct DedupKeys {
    canonical_url: Option<String>,
    source_ids: Vec<(String, String)>,
    title: String,
    start_date: Option<NaiveDate>,
    organizer: Option<String>,
    location: Option<String>,
}

impl DedupKeys {
    fn from_event(event: &Event) -> DedupKeys {
        let source_ids = event
            .sources
            .iter()
            .filter_map(|s| {
                s.native_id
                    .as_ref()
                    .map(|id| (s.source_id.clone(), id.clone()))
            })
            .collect();
        DedupKeys {
            canonical_url: event.url.as_ref().map(canonicalize_url),
            source_ids,
            title: normalize_name(&event.title),
            start_date: start_date(event),
            organizer: organizer_key(event),
            location: event.location.as_ref().and_then(location_key),
        }
    }

    fn duplicate_signal(&self, other: &DedupKeys) -> Option<DedupSignal> {
        DedupSignal::PRIORITY
            .into_iter()
            .find(|&sig| self.are_duplicates(other, sig))
    }

    fn are_duplicates(&self, other: &DedupKeys, signal: DedupSignal) -> bool {
        match signal {
            DedupSignal::CanonicalUrl => match (&self.canonical_url, &other.canonical_url) {
                (Some(ua), Some(ub)) => ua == ub,
                _ => false,
            },
            DedupSignal::SourceCanonicalId => self.source_ids.iter().any(|(sa, ida)| {
                other
                    .source_ids
                    .iter()
                    .any(|(sb, idb)| sb == sa && idb == ida)
            }),
            DedupSignal::TitleDateOrganizer => {
                let (Some(da), Some(db)) = (self.start_date, other.start_date) else {
                    return false;
                };
                da == db
                    && self.title == other.title
                    && match (&self.organizer, &other.organizer) {
                        (Some(oa), Some(ob)) => oa == ob,
                        _ => false,
                    }
            }
            DedupSignal::TitleDateLocation => {
                let (Some(da), Some(db)) = (self.start_date, other.start_date) else {
                    return false;
                };
                da == db
                    && self.title == other.title
                    && match (&self.location, &other.location) {
                        (Some(la), Some(lb)) => la == lb,
                        _ => false,
                    }
            }
        }
    }
}

/// Decide whether two events are duplicates under the given signal.
///
/// Conservative: signals 3 and 4 (TitleDateOrganizer / TitleDateLocation)
/// require the start dates to be equal. Signals 1 and 2 (URL / source ID) are
/// strong enough to merge regardless of date — a source that republishes the
/// same event id at a changed date is treated as a corrected record of the
/// same event.
pub fn are_duplicates(a: &Event, b: &Event, signal: DedupSignal) -> bool {
    match signal {
        DedupSignal::CanonicalUrl => match (a.url.as_ref(), b.url.as_ref()) {
            (Some(ua), Some(ub)) => canonicalize_url(ua) == canonicalize_url(ub),
            _ => false,
        },
        DedupSignal::SourceCanonicalId => {
            // Two events share a source canonical id iff some source of `a`
            // and some source of `b` declare the same (source_id, native_id)
            // pair with a non-empty native_id.
            a.sources
                .iter()
                .filter_map(|s| {
                    s.native_id
                        .as_ref()
                        .map(|id| (s.source_id.as_str(), id.as_str()))
                })
                .any(|(sa, ida)| {
                    b.sources.iter().any(|sb| {
                        sb.native_id
                            .as_deref()
                            .is_some_and(|idb| sb.source_id == sa && idb == ida)
                    })
                })
        }
        DedupSignal::TitleDateOrganizer => {
            let (Some(da), Some(db)) = (start_date(a), start_date(b)) else {
                return false;
            };
            if da != db {
                return false;
            }
            if normalize_name(&a.title) != normalize_name(&b.title) {
                return false;
            }
            organizer_key(a).is_some_and(|oa| organizer_key(b).is_some_and(|ob| oa == ob))
        }
        DedupSignal::TitleDateLocation => {
            let (Some(da), Some(db)) = (start_date(a), start_date(b)) else {
                return false;
            };
            if da != db {
                return false;
            }
            if normalize_name(&a.title) != normalize_name(&b.title) {
                return false;
            }
            match (a.location.as_ref(), b.location.as_ref()) {
                (Some(la), Some(lb)) => {
                    location_key(la).is_some_and(|ka| location_key(lb).is_some_and(|kb| ka == kb))
                }
                _ => false,
            }
        }
    }
}

/// Strongest signal (if any) under which `a` and `b` are duplicates. Tries
/// signals in §25 priority order and returns the first match.
pub fn duplicate_signal(a: &Event, b: &Event) -> Option<DedupSignal> {
    DedupSignal::PRIORITY
        .into_iter()
        .find(|&sig| are_duplicates(a, b, sig))
}

/// Merge two duplicate events into one, preferring the higher-scored event as
/// the primary carrier of scalar fields. Collection fields (sources, media,
/// talks, people, topics) are unioned with dedup. Scalar fields (url, location,
/// description, date) that are absent on the primary are filled from the
/// secondary so genuine data from the lower-scored duplicate is not lost.
///
/// The returned event keeps the earliest `first_seen_at` and the latest
/// `last_seen_at`.
pub fn merge_events(primary: Event, secondary: Event) -> Event {
    let (mut keep, other) = if primary.score >= secondary.score {
        (primary, secondary)
    } else {
        (secondary, primary)
    };

    keep.sources = union_sources(keep.sources, other.sources);
    keep.media = union_media(keep.media, other.media);
    keep.talks = union_talks(keep.talks, other.talks);
    keep.people = union_people(keep.people, other.people);
    keep.topics = union_topics(keep.topics, other.topics);

    if keep.url.is_none() {
        keep.url = other.url;
    }
    if keep.location.is_none() {
        keep.location = other.location;
    }
    if keep.description.is_none() {
        keep.description = other.description;
    }
    if keep.date.start.is_none() && other.date.start.is_some() {
        keep.date = other.date;
    }

    keep.first_seen_at = earliest(keep.first_seen_at, other.first_seen_at);
    keep.last_seen_at = latest(keep.last_seen_at, other.last_seen_at);

    keep
}

fn union_sources(a: Vec<SourceEvidence>, b: Vec<SourceEvidence>) -> Vec<SourceEvidence> {
    let mut out = a;
    let mut seen: HashSet<(String, String)> = out
        .iter()
        .map(|s| (s.source_id.clone(), s.source_url.to_string()))
        .collect();
    for s in b {
        let key = (s.source_id.clone(), s.source_url.to_string());
        if seen.insert(key) {
            out.push(s);
        }
    }
    out
}

fn union_media(a: Vec<MediaResource>, b: Vec<MediaResource>) -> Vec<MediaResource> {
    let mut out = a;
    let mut seen: HashSet<Url> = out.iter().map(|m| m.url.clone()).collect();
    for m in b {
        if seen.insert(m.url.clone()) {
            out.push(m);
        }
    }
    out
}

fn union_talks(a: Vec<Talk>, b: Vec<Talk>) -> Vec<Talk> {
    let mut out = a;
    let mut seen: HashSet<String> = out.iter().map(|t| t.id.0.clone()).collect();
    for t in b {
        if seen.insert(t.id.0.clone()) {
            out.push(t);
        }
    }
    out
}

fn union_people(a: Vec<PersonHit>, b: Vec<PersonHit>) -> Vec<PersonHit> {
    let mut out = a;
    let mut seen: HashSet<(String, PersonRole)> = out
        .iter()
        .map(|p| (p.canonical_name.clone(), p.role))
        .collect();
    for p in b {
        if seen.insert((p.canonical_name.clone(), p.role)) {
            out.push(p);
        }
    }
    out
}

fn union_topics(a: Vec<TopicMatch>, b: Vec<TopicMatch>) -> Vec<TopicMatch> {
    let mut out = a;
    let mut seen: HashSet<String> = out.iter().map(|t| t.topic_id.clone()).collect();
    for t in b {
        if seen.insert(t.topic_id.clone()) {
            out.push(t);
        }
    }
    out
}

fn earliest(
    a: Option<chrono::DateTime<chrono::Utc>>,
    b: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

fn latest(
    a: Option<chrono::DateTime<chrono::Utc>>,
    b: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

/// Deduplicate a batch of events. Events are processed in a stable order (by
/// id) and merged greedily: each event is compared against existing clusters'
/// representatives using the §25 signal priority, and merged into the first
/// cluster it matches. Events that match no cluster start their own.
///
/// This is a single-pass O(n²) algorithm — adequate for the v0.1 batch sizes
/// (low thousands of events per scan). A hash-indexed pass is a later
/// optimization that must not change merge decisions.
pub fn dedup_events(events: Vec<Event>) -> Vec<Event> {
    // Stable sort by id so the cluster representative is deterministic
    // regardless of input order.
    let mut sorted: Vec<Event> = events;
    sorted.sort_by(|a, b| a.id.0.cmp(&b.id.0));

    let mut clusters: Vec<Event> = Vec::with_capacity(sorted.len());
    let mut cluster_keys: Vec<DedupKeys> = Vec::with_capacity(sorted.len());
    for ev in sorted {
        let incoming = DedupKeys::from_event(&ev);
        let mut current = Some(ev);
        let mut current_keys = Some(incoming);
        for (rep, rep_keys) in clusters.iter_mut().zip(cluster_keys.iter_mut()) {
            let (Some(remaining), Some(remaining_keys)) = (current.take(), current_keys.take())
            else {
                break;
            };
            if rep_keys.duplicate_signal(&remaining_keys).is_some() {
                let old = rep.clone();
                *rep = merge_events(old, remaining);
                *rep_keys = DedupKeys::from_event(rep);
            } else {
                current = Some(remaining);
                current_keys = Some(remaining_keys);
            }
        }
        if let Some(remaining) = current {
            clusters.push(remaining);
            cluster_keys.push(current_keys.unwrap());
        }
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::{DatePrecision, EventDate};
    use crate::model::{
        AccessInfo, EventId, EventStatus, EventType, OnlineAvailability, PublicAccess,
        SourceEvidence,
    };
    use url::Url;

    fn src(source_id: &str, url: &str, native_id: Option<&str>) -> SourceEvidence {
        SourceEvidence {
            source_id: source_id.to_string(),
            source_url: Url::parse(url).unwrap(),
            evidence: None,
            captured_at: None,
            native_id: native_id.map(str::to_string),
        }
    }

    fn event(
        id: &str,
        title: &str,
        url: Option<&str>,
        start: Option<chrono::NaiveDate>,
        sources: Vec<SourceEvidence>,
    ) -> Event {
        Event {
            id: EventId(id.to_string()),
            title: title.to_string(),
            url: url.map(|u| Url::parse(u).unwrap()),
            event_type: EventType::Conference,
            status: EventStatus::Unknown,
            date: EventDate {
                start: start.map(crate::date::DateTimeOrDate::Date),
                end: None,
                timezone: None,
                original_text: String::new(),
                precision: start.map_or(DatePrecision::Unknown, |_| DatePrecision::Day),
            },
            location: None,
            description: None,
            topics: Vec::new(),
            people: Vec::new(),
            talks: Vec::new(),
            media: Vec::new(),
            access: AccessInfo {
                access: PublicAccess::Unknown,
                online: OnlineAvailability::Unknown,
            },
            sources,
            score: 0.0,
            score_components: crate::ranking::ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        }
    }

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn canonicalize_url_strips_fragment_query_and_case() {
        let a = canonicalize_url(&Url::parse("https://Example.com/events/42").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/events/42#top").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_url_strips_trailing_slash_and_default_port() {
        let a = canonicalize_url(&Url::parse("https://host.com:443/path/").unwrap());
        let b = canonicalize_url(&Url::parse("https://host.com/path").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_url_strips_www_prefix() {
        let a = canonicalize_url(&Url::parse("https://www.example.com/e1").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e1").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_url_preserves_meaningful_params() {
        let a = canonicalize_url(&Url::parse("https://example.com/e?session=abc").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e?session=xyz").unwrap());
        assert_ne!(a, b, "distinct session params must not canonicalize equal");
        let c = canonicalize_url(&Url::parse("https://example.com/e?session=abc").unwrap());
        assert_eq!(a, c, "same session param must canonicalize equal");
    }

    #[test]
    fn canonicalize_url_drops_tracking_params() {
        let a = canonicalize_url(
            &Url::parse("https://example.com/e?utm_source=nl&fbclid=xyz").unwrap(),
        );
        let b = canonicalize_url(&Url::parse("https://example.com/e").unwrap());
        assert_eq!(a, b, "tracking params must be dropped");
    }

    // Generic keys `ref`, `source`, `ver` are NOT tracking params — they may
    // distinguish distinct calendar references, so they must be preserved to
    // avoid over-merging (§47).
    #[test]
    fn canonicalize_url_preserves_generic_ref_source_ver() {
        let a = canonicalize_url(&Url::parse("https://example.com/e?ref=calendar_a").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e?ref=calendar_b").unwrap());
        assert_ne!(a, b, "distinct ref values must not canonicalize equal");

        let c = canonicalize_url(&Url::parse("https://example.com/e?source=feed_a").unwrap());
        let d = canonicalize_url(&Url::parse("https://example.com/e?source=feed_b").unwrap());
        assert_ne!(c, d, "distinct source values must not canonicalize equal");

        let e = canonicalize_url(&Url::parse("https://example.com/e?ver=1").unwrap());
        let f = canonicalize_url(&Url::parse("https://example.com/e?ver=2").unwrap());
        assert_ne!(e, f, "distinct ver values must not canonicalize equal");

        // A tracking param alongside a generic ref: tracking dropped, ref kept.
        let g = canonicalize_url(
            &Url::parse("https://example.com/e?utm_source=nl&ref=calendar_a").unwrap(),
        );
        let h = canonicalize_url(&Url::parse("https://example.com/e?ref=calendar_a").unwrap());
        assert_eq!(g, h, "utm_source dropped, ref preserved");
    }

    #[test]
    fn canonicalize_url_sorts_query_params() {
        let a = canonicalize_url(&Url::parse("https://example.com/e?b=2&a=1").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e?a=1&b=2").unwrap());
        assert_eq!(
            a, b,
            "params must be sorted for deterministic canonicalization"
        );
    }

    #[test]
    fn canonicalize_url_preserves_tracking_alongside_meaningful() {
        let a = canonicalize_url(
            &Url::parse("https://example.com/e?utm_source=nl&session=42").unwrap(),
        );
        let b = canonicalize_url(&Url::parse("https://example.com/e?session=42").unwrap());
        assert_eq!(a, b, "tracking dropped, meaningful preserved");
    }

    #[test]
    fn domain_key_handles_ac_uk() {
        let url = Url::parse("https://www.maths.ox.ac.uk/events").unwrap();
        assert_eq!(domain_key(&url).unwrap(), "ox.ac.uk");
    }

    #[test]
    fn domain_key_handles_co_jp() {
        let url = Url::parse("https://example.co.jp/page").unwrap();
        assert_eq!(domain_key(&url).unwrap(), "example.co.jp");
    }

    #[test]
    fn domain_key_plain_two_label_tld() {
        let url = Url::parse("https://www.claymath.org/feed").unwrap();
        assert_eq!(domain_key(&url).unwrap(), "claymath.org");
    }

    #[test]
    fn canonical_url_match_merges() {
        let a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        assert!(are_duplicates(&a, &b, DedupSignal::CanonicalUrl));
        assert!(duplicate_signal(&a, &b) == Some(DedupSignal::CanonicalUrl));
    }

    #[test]
    fn different_urls_do_not_match_on_canonical_url() {
        let a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk",
            Some("https://x.com/e2"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::CanonicalUrl));
    }

    #[test]
    fn source_canonical_id_match_merges_across_different_urls() {
        let a = event(
            "a",
            "Indico Talk",
            Some("https://indico.com/event/1"),
            Some(date(2026, 8, 9)),
            vec![src("indico", "https://indico.com/api", Some("1"))],
        );
        let b = event(
            "b",
            "Indico Talk",
            Some("https://mirror.com/event/1"),
            Some(date(2026, 8, 9)),
            vec![src("indico", "https://mirror.com/api", Some("1"))],
        );
        assert!(are_duplicates(&a, &b, DedupSignal::SourceCanonicalId));
        assert!(duplicate_signal(&a, &b) == Some(DedupSignal::SourceCanonicalId));
    }

    #[test]
    fn source_canonical_id_requires_matching_source_id() {
        let a = event(
            "a",
            "Talk",
            None,
            Some(date(2026, 8, 9)),
            vec![src("indico", "https://indico.com", Some("1"))],
        );
        let b = event(
            "b",
            "Talk",
            None,
            Some(date(2026, 8, 9)),
            vec![src("other", "https://other.com", Some("1"))],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::SourceCanonicalId));
    }

    #[test]
    fn same_title_date_organizer_domain_merges() {
        let a = event(
            "a",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://math.mit.edu/events", None)],
        );
        let b = event(
            "b",
            "algebraic geometry conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://math.mit.edu/calendar", None)],
        );
        assert!(are_duplicates(&a, &b, DedupSignal::TitleDateOrganizer));
    }

    #[test]
    fn same_title_different_date_does_not_merge_on_organizer() {
        let a = event(
            "a",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://math.mit.edu/events", None)],
        );
        let b = event(
            "b",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 9, 9)),
            vec![src("s2", "https://math.mit.edu/calendar", None)],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::TitleDateOrganizer));
        assert!(duplicate_signal(&a, &b).is_none());
    }

    #[test]
    fn different_title_does_not_merge() {
        let a = event(
            "a",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://math.mit.edu/events", None)],
        );
        let b = event(
            "b",
            "Number Theory Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://math.mit.edu/calendar", None)],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::TitleDateOrganizer));
        assert!(duplicate_signal(&a, &b).is_none());
    }

    #[test]
    fn dedup_events_merges_a_pair() {
        let a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        let out = dedup_events(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sources.len(), 2);
    }

    #[test]
    fn dedup_events_keeps_distinct_events_separate() {
        let a = event(
            "a",
            "Talk A",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk B",
            Some("https://x.com/e2"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        let out = dedup_events(vec![a, b]);
        assert_eq!(out.len(), 2);
    }

    // CORE-18: merge_events fills scalar gaps from the lower-scored event.
    #[test]
    fn merge_events_fills_scalar_gaps() {
        let mut a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        a.score = 10.0;
        a.description = Some("Description from A".into());
        a.location = Some(Location {
            name: "MIT".into(),
            city: Some("Cambridge".into()),
            country: None,
            venue: None,
        });

        let mut b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        b.score = 5.0;
        b.description = None;
        b.location = None;

        let merged = merge_events(a, b);
        assert_eq!(merged.description.as_deref(), Some("Description from A"));
        assert!(
            merged.location.is_some(),
            "location preserved from higher-scored"
        );
    }

    #[test]
    fn merge_events_fills_from_secondary_when_primary_lacks() {
        let mut a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        a.score = 10.0;
        a.description = None;
        a.location = None;

        let mut b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        b.score = 5.0;
        b.description = Some("Description from B".into());
        b.location = Some(Location {
            name: "MIT".into(),
            city: None,
            country: None,
            venue: None,
        });

        let merged = merge_events(a, b);
        assert_eq!(
            merged.description.as_deref(),
            Some("Description from B"),
            "description filled from secondary when primary lacks it"
        );
        assert!(
            merged.location.is_some(),
            "location filled from secondary when primary lacks it"
        );
    }
}
