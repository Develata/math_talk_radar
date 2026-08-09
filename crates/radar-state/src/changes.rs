//! Change-detection events (§23). Emitted by comparing the current scan's
//! canonical fingerprints against the previous state.
//!
//! The canonical baseline (§23): an event first seen with `media = []` and
//! re-seen with a new video must produce a [`ChangeKind::MediaAdded`]. An
//! unchanged event produces no records (STATE-002).
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use radar_core::{Event, EventId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    EventAdded,
    EventUpdated,
    ScheduleAdded,
    SpeakerAdded,
    LivestreamAdded,
    MediaAdded,
    MediaRemoved,
    EventCancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub kind: ChangeKind,
    pub event_id: EventId,
    pub detected_at: DateTime<Utc>,
    #[serde(default)]
    pub detail: Option<String>,
}

impl ChangeRecord {
    fn new(
        kind: ChangeKind,
        event_id: EventId,
        now: DateTime<Utc>,
        detail: Option<String>,
    ) -> Self {
        Self {
            kind,
            event_id,
            detected_at: now,
            detail,
        }
    }
}

/// Compare a previous scan's persisted events against the current scan and
/// emit change records (§23). Events are matched by [`EventId`].
///
/// Emits:
/// - [`ChangeKind::EventAdded`] for events present in `current` but not
///   `previous`.
/// - [`ChangeKind::EventCancelled`] for events present in `previous` but not
///   `current`.
/// - [`ChangeKind::MediaAdded`] for each media URL present in the current
///   event but absent from the previous event (canonical baseline §23).
/// - [`ChangeKind::MediaRemoved`] for each media URL that disappeared.
/// - [`ChangeKind::LivestreamAdded`] when a newly-added medium is a
///   livestream (a more specific form of MediaAdded — emitted instead, not
///   in addition).
///
/// Unchanged events produce no records (STATE-002). The output is sorted by
/// `(event_id, kind)` so identical inputs yield identical output regardless of
/// map iteration order (§11 determinism).
pub fn detect_changes(
    previous: &[Event],
    current: &[Event],
    now: DateTime<Utc>,
) -> Vec<ChangeRecord> {
    let prev_by_id: HashMap<&EventId, &Event> = previous.iter().map(|e| (&e.id, e)).collect();
    let curr_by_id: HashMap<&EventId, &Event> = current.iter().map(|e| (&e.id, e)).collect();

    let mut records: Vec<ChangeRecord> = Vec::new();

    let prev_ids: HashSet<&EventId> = prev_by_id.keys().copied().collect();
    let curr_ids: HashSet<&EventId> = curr_by_id.keys().copied().collect();

    for id in curr_ids.difference(&prev_ids) {
        records.push(ChangeRecord::new(
            ChangeKind::EventAdded,
            (*id).clone(),
            now,
            None,
        ));
    }
    for id in prev_ids.difference(&curr_ids) {
        records.push(ChangeRecord::new(
            ChangeKind::EventCancelled,
            (*id).clone(),
            now,
            None,
        ));
    }
    for id in curr_ids.intersection(&prev_ids) {
        let prev = prev_by_id[id];
        let curr = curr_by_id[id];
        let prev_urls: HashSet<&str> = prev.media.iter().map(|m| m.url.as_str()).collect();
        let curr_urls: HashSet<&str> = curr.media.iter().map(|m| m.url.as_str()).collect();

        for m in &curr.media {
            if !prev_urls.contains(m.url.as_str()) {
                let kind = if matches!(m.media_type, radar_core::MediaType::Livestream) {
                    ChangeKind::LivestreamAdded
                } else {
                    ChangeKind::MediaAdded
                };
                records.push(ChangeRecord::new(
                    kind,
                    curr.id.clone(),
                    now,
                    Some(m.url.as_str().to_string()),
                ));
            }
        }
        for m in &prev.media {
            if !curr_urls.contains(m.url.as_str()) {
                records.push(ChangeRecord::new(
                    ChangeKind::MediaRemoved,
                    curr.id.clone(),
                    now,
                    Some(m.url.as_str().to_string()),
                ));
            }
        }
    }

    records.sort_by(|a, b| (&a.event_id.0, a.kind as u8).cmp(&(&b.event_id.0, b.kind as u8)));
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use radar_core::{
        AccessInfo, EventDate, EventId, EventStatus, EventType, MediaId, MediaResource, MediaType,
        OnlineAvailability, PublicAccess, SourceEvidence,
    };
    use url::Url;

    fn now() -> DateTime<Utc> {
        DateTime::<Utc>::from_naive_utc_and_offset(
            NaiveDate::from_ymd_opt(2026, 8, 9)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            Utc,
        )
    }

    fn src() -> SourceEvidence {
        SourceEvidence {
            source_id: "s1".into(),
            source_url: Url::parse("https://example.com/feed").unwrap(),
            evidence: None,
            captured_at: None,
            native_id: None,
        }
    }

    fn event(id: &str, media: Vec<MediaResource>) -> Event {
        Event {
            id: EventId(id.into()),
            title: "T".into(),
            url: None,
            event_type: EventType::Conference,
            status: EventStatus::Unknown,
            date: EventDate {
                start: None,
                end: None,
                timezone: None,
                original_text: String::new(),
                precision: radar_core::DatePrecision::Unknown,
            },
            location: None,
            description: None,
            topics: Vec::new(),
            people: Vec::new(),
            talks: Vec::new(),
            media,
            access: AccessInfo {
                access: PublicAccess::Unknown,
                online: OnlineAvailability::Unknown,
            },
            sources: vec![src()],
            score: 0.0,
            score_components: radar_core::ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        }
    }

    fn video(url: &str) -> MediaResource {
        MediaResource {
            id: MediaId("m1".into()),
            media_type: MediaType::Video,
            title: None,
            url: Url::parse(url).unwrap(),
            platform: None,
            public_access: PublicAccess::Open,
            published_at: None,
            source: src(),
        }
    }

    #[test]
    fn empty_scans_produce_no_records() {
        let records = detect_changes(&[], &[], now());
        assert!(records.is_empty());
    }

    #[test]
    fn new_event_emits_event_added() {
        let curr = vec![event("e1", vec![])];
        let records = detect_changes(&[], &curr, now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::EventAdded);
        assert_eq!(records[0].event_id.0, "e1");
    }

    #[test]
    fn unchanged_event_emits_nothing() {
        let prev = event("e1", vec![]);
        let curr = event("e1", vec![]);
        let records = detect_changes(&[prev], &[curr], now());
        assert!(records.is_empty());
    }

    #[test]
    fn canonical_baseline_media_added() {
        let prev = vec![event("e1", vec![])];
        let curr = vec![event("e1", vec![video("https://youtube.com/v/1")])];
        let records = detect_changes(&prev, &curr, now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::MediaAdded);
        assert_eq!(
            records[0].detail.as_deref(),
            Some("https://youtube.com/v/1")
        );
    }

    #[test]
    fn media_removed_emits_media_removed() {
        let prev = vec![event("e1", vec![video("https://youtube.com/v/1")])];
        let curr = vec![event("e1", vec![])];
        let records = detect_changes(&prev, &curr, now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::MediaRemoved);
    }

    #[test]
    fn disappeared_event_emits_event_cancelled() {
        let prev = vec![event("e1", vec![])];
        let records = detect_changes(&prev, &[], now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::EventCancelled);
    }
}
