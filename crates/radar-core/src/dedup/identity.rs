//! Exact identity keys sanctioned by §25. No ranking or presentation policy.
use crate::model::{Event, Location};
use crate::normalize::{canonicalize_url, normalize_name};
use crate::people::PersonRole;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use url::Url;

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
pub(super) fn domain_key(url: &Url) -> Option<String> {
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
/// and used for exact-key lookup in `dedup_events`. The test-only reference
/// also reuses these fields across its linear cluster scan.
/// Must be recomputed for a cluster representative after a merge, since the
/// merge can union in new sources, location, or people.
pub(super) struct DedupKeys {
    canonical_url: Option<String>,
    source_ids: Vec<(String, String)>,
    title: String,
    start_date: Option<NaiveDate>,
    organizer: Option<String>,
    location: Option<String>,
}

impl DedupKeys {
    pub(super) fn from_event(event: &Event) -> DedupKeys {
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

    pub(super) fn identity_keys(&self) -> Vec<IdentityKey> {
        let mut keys = Vec::with_capacity(self.source_ids.len() + 3);
        if let Some(url) = &self.canonical_url {
            keys.push(IdentityKey::Url(url.clone()));
        }
        keys.extend(
            self.source_ids
                .iter()
                .cloned()
                .map(|(source, native)| IdentityKey::Native(source, native)),
        );
        if let Some(date) = self.start_date {
            if let Some(organizer) = &self.organizer {
                keys.push(IdentityKey::Organizer(
                    self.title.clone(),
                    date,
                    organizer.clone(),
                ));
            }
            if let Some(location) = &self.location {
                keys.push(IdentityKey::Location(
                    self.title.clone(),
                    date,
                    location.clone(),
                ));
            }
        }
        keys
    }

    #[cfg(test)]
    pub(super) fn duplicate_signal(&self, other: &DedupKeys) -> Option<DedupSignal> {
        DedupSignal::PRIORITY
            .into_iter()
            .find(|&sig| self.are_duplicates(other, sig))
    }

    #[cfg(test)]
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

pub(super) fn scalar_identity_keys(event: &Event) -> Vec<IdentityKey> {
    DedupKeys {
        canonical_url: event.url.as_ref().map(canonicalize_url),
        source_ids: Vec::new(),
        title: normalize_name(&event.title),
        start_date: start_date(event),
        organizer: organizer_key(event),
        location: event.location.as_ref().and_then(location_key),
    }
    .identity_keys()
}

pub(super) fn native_identity_keys(event: &Event) -> impl Iterator<Item = IdentityKey> + '_ {
    event.sources.iter().filter_map(|s| {
        s.native_id
            .as_ref()
            .map(|id| IdentityKey::Native(s.source_id.clone(), id.clone()))
    })
}

/// An identity signal is an exact key, not a fuzzy/transitive relationship.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum IdentityKey {
    Url(String),
    Native(String, String),
    Organizer(String, NaiveDate, String),
    Location(String, NaiveDate, String),
}
