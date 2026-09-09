//! Canonical ordering and evidence union. Collection caches live only as long
//! as one dedup pass; stored events retain the existing public representation.
use crate::model::{Event, MediaResource, SourceEvidence, Talk};
use crate::people::{PersonHit, PersonRole};
use crate::topics::TopicMatch;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use url::Url;

/// Ranking-independent canonical order. IDs already encode normalized title
/// and URL; provenance breaks same-ID multi-source ties without copying strings.
/// Exact key ties retain input order. Neither scores nor ranking reasons enter
/// identity, and completeness is filled by merge rather than used as a weight.
pub(super) fn canonical_cmp(a: &Event, b: &Event) -> Ordering {
    a.id.0
        .cmp(&b.id.0)
        .then_with(|| a.url.cmp(&b.url))
        .then_with(|| {
            fn key(s: &SourceEvidence) -> (&str, &str, Option<&str>) {
                (&s.source_id, s.source_url.as_str(), s.native_id.as_deref())
            }
            a.sources.iter().map(key).cmp(b.sources.iter().map(key))
        })
        .then_with(|| a.title.cmp(&b.title))
        .then_with(|| a.description.cmp(&b.description))
}

/// Merge duplicate events using the ranking-independent canonical order.
/// Collections are unioned; absent scalar fields are filled from the secondary.
/// Keeps the earliest first_seen_at and latest last_seen_at.
pub fn merge_events(mut primary: Event, secondary: Event) -> Event {
    merge_in_place(&mut primary, secondary);
    primary
}

pub(super) fn merge_in_place(keep: &mut Event, mut other: Event) {
    if canonical_cmp(keep, &other).is_gt() {
        std::mem::swap(keep, &mut other);
    }
    keep.sources = union_sources(std::mem::take(&mut keep.sources), other.sources);
    keep.media = union_media(std::mem::take(&mut keep.media), other.media);
    keep.talks = union_talks(std::mem::take(&mut keep.talks), other.talks);
    keep.people = union_people(std::mem::take(&mut keep.people), other.people);
    keep.topics = union_topics(std::mem::take(&mut keep.topics), other.topics);

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
    let mut index: std::collections::HashMap<String, usize> = out
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.0.clone(), i))
        .collect();
    for t in b {
        if let Some(&pos) = index.get(&t.id.0) {
            merge_talk(&mut out[pos], t);
        } else {
            index.insert(t.id.0.clone(), out.len());
            out.push(t);
        }
    }
    out
}

/// Merge two talks sharing the same ID. The primary carries scalar fields;
/// the secondary fills gaps and unions collection fields (speakers, media,
/// topics). H07: previously `union_talks` dropped the secondary entirely on
/// ID collision, losing its speakers/media/abstract when the primary lacked
/// them.
fn merge_talk(keep: &mut Talk, secondary: Talk) {
    keep.speaker = union_people(std::mem::take(&mut keep.speaker), secondary.speaker);
    keep.media = union_media(std::mem::take(&mut keep.media), secondary.media);
    keep.topics = union_topics(std::mem::take(&mut keep.topics), secondary.topics);
    if keep.date_time.is_none() {
        keep.date_time = secondary.date_time;
    }
    if keep.abstract_text.is_none() {
        keep.abstract_text = secondary.abstract_text;
    }
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

/// Membership caches are allocated only for clusters that actually merge.
/// They avoid rebuilding a growing corpus of provenance/media on every input.
pub(super) struct MergeIndex {
    sources: HashSet<(String, Url)>,
    media: HashSet<Url>,
    talks: HashMap<String, usize>,
    people: HashSet<(String, PersonRole)>,
    topics: HashSet<String>,
}

impl MergeIndex {
    pub(super) fn new(event: &Event) -> Self {
        Self {
            sources: event
                .sources
                .iter()
                .map(|s| (s.source_id.clone(), s.source_url.clone()))
                .collect(),
            media: event.media.iter().map(|m| m.url.clone()).collect(),
            talks: event
                .talks
                .iter()
                .enumerate()
                .map(|(i, t)| (t.id.0.clone(), i))
                .collect(),
            people: event
                .people
                .iter()
                .map(|p| (p.canonical_name.clone(), p.role))
                .collect(),
            topics: event.topics.iter().map(|t| t.topic_id.clone()).collect(),
        }
    }

    pub(super) fn merge(&mut self, keep: &mut Event, mut other: Event) {
        if canonical_cmp(keep, &other).is_gt() {
            std::mem::swap(keep, &mut other);
            *self = Self::new(keep);
        }
        for source in other.sources {
            if self
                .sources
                .insert((source.source_id.clone(), source.source_url.clone()))
            {
                keep.sources.push(source);
            }
        }
        for medium in other.media {
            if self.media.insert(medium.url.clone()) {
                keep.media.push(medium);
            }
        }
        for talk in other.talks {
            if let Some(&i) = self.talks.get(&talk.id.0) {
                merge_talk(&mut keep.talks[i], talk);
            } else {
                self.talks.insert(talk.id.0.clone(), keep.talks.len());
                keep.talks.push(talk);
            }
        }
        for person in other.people {
            if self
                .people
                .insert((person.canonical_name.clone(), person.role))
            {
                keep.people.push(person);
            }
        }
        for topic in other.topics {
            if self.topics.insert(topic.topic_id.clone()) {
                keep.topics.push(topic);
            }
        }
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
    }
}
