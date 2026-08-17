//! Change-detection events (§23). Emitted by comparing the current scan's
//! canonical fingerprints against the previous state.
//!
//! The canonical baseline (§23): an event first seen with `media = []` and
//! re-seen with a new video must produce a [`ChangeKind::MediaAdded`]. An
//! unchanged event produces no records (STATE-002).
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use radar_core::{Event, EventDate, EventId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

impl ChangeKind {
    /// Stable snake_case string for composite-key construction in
    /// `CHANGE_LOG` (ADR-0011 §3). Must match `#[serde(rename_all)]`.
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::EventAdded => "event_added",
            ChangeKind::EventUpdated => "event_updated",
            ChangeKind::ScheduleAdded => "schedule_added",
            ChangeKind::SpeakerAdded => "speaker_added",
            ChangeKind::LivestreamAdded => "livestream_added",
            ChangeKind::MediaAdded => "media_added",
            ChangeKind::MediaRemoved => "media_removed",
            ChangeKind::EventCancelled => "event_cancelled",
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
/// - [`ChangeKind::EventUpdated`] when a persisted event's title, date,
///   location, or description changed.
/// - [`ChangeKind::ScheduleAdded`] when new talks appear that were not in the
///   previous scan.
/// - [`ChangeKind::SpeakerAdded`] when new speakers (people or talk speakers)
///   appear that were not in the previous scan.
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

        if is_event_updated(prev, curr) {
            records.push(ChangeRecord::new(
                ChangeKind::EventUpdated,
                curr.id.clone(),
                now,
                None,
            ));
        }

        for new_talk_id in new_talk_ids(prev, curr) {
            records.push(ChangeRecord::new(
                ChangeKind::ScheduleAdded,
                curr.id.clone(),
                now,
                Some(new_talk_id),
            ));
        }

        for new_speaker in new_speakers(prev, curr) {
            records.push(ChangeRecord::new(
                ChangeKind::SpeakerAdded,
                curr.id.clone(),
                now,
                Some(new_speaker),
            ));
        }

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

fn is_event_updated(prev: &Event, curr: &Event) -> bool {
    prev.title != curr.title
        || !dates_semantically_equal(&prev.date, &curr.date)
        || prev.location != curr.location
        || prev.description != curr.description
}

/// Compare only the semantically meaningful fields of [`EventDate`].
/// `original_text` and `precision` are parser artifacts: the same date can be
/// rendered as "Aug 12" or "August 12th" across scans, flipping those fields
/// without changing the event. Comparing them would produce spurious
/// `event_updated` records (§47 false-positive risk).
fn dates_semantically_equal(a: &EventDate, b: &EventDate) -> bool {
    a.start == b.start && a.end == b.end && a.timezone == b.timezone
}

fn new_talk_ids(prev: &Event, curr: &Event) -> Vec<String> {
    let prev_ids: HashSet<&str> = prev.talks.iter().map(|t| t.id.0.as_str()).collect();
    curr.talks
        .iter()
        .filter(|t| !prev_ids.contains(t.id.0.as_str()))
        .map(|t| t.id.0.clone())
        .collect()
}

fn new_speakers(prev: &Event, curr: &Event) -> Vec<String> {
    let prev_names: HashSet<String> = prev
        .people
        .iter()
        .filter(|p| p.role == radar_core::PersonRole::Speaker)
        .map(|p| p.canonical_name.clone())
        .collect();
    let mut added: Vec<String> = Vec::new();
    let mut added_set: HashSet<String> = HashSet::new();
    for p in curr
        .people
        .iter()
        .filter(|p| p.role == radar_core::PersonRole::Speaker)
    {
        if !prev_names.contains(&p.canonical_name) && added_set.insert(p.canonical_name.clone()) {
            added.push(p.canonical_name.clone());
        }
    }
    let prev_talk_speakers: HashSet<String> = prev
        .talks
        .iter()
        .flat_map(|t| t.speaker.iter())
        .map(|s| s.canonical_name.clone())
        .collect();
    for t in &curr.talks {
        for s in &t.speaker {
            if !prev_talk_speakers.contains(&s.canonical_name)
                && added_set.insert(s.canonical_name.clone())
            {
                added.push(s.canonical_name.clone());
            }
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use radar_core::{
        AccessInfo, EventDate, EventId, EventStatus, EventType, MediaId, MediaResource, MediaType,
        OnlineAvailability, PersonHit, PersonRole, PublicAccess, SourceEvidence, Talk, TalkId,
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

    fn speaker(name: &str) -> PersonHit {
        PersonHit {
            canonical_name: name.into(),
            matched_text: name.into(),
            role: PersonRole::Speaker,
            evidence: None,
            confidence: 1.0,
            scholar_tags: Vec::new(),
        }
    }

    fn talk(id: &str, speaker_name: Option<&str>) -> Talk {
        Talk {
            id: TalkId(id.into()),
            title: format!("Talk {id}"),
            speaker: speaker_name.into_iter().map(speaker).collect(),
            date_time: None,
            abstract_text: None,
            topics: Vec::new(),
            media: Vec::new(),
            source: src(),
        }
    }

    fn event_with_title(id: &str, title: &str) -> Event {
        let mut e = event(id, Vec::new());
        e.title = title.into();
        e
    }

    fn event_with_talks(id: &str, talks: Vec<Talk>) -> Event {
        let mut e = event(id, Vec::new());
        e.talks = talks;
        e
    }

    fn event_with_people(id: &str, people: Vec<PersonHit>) -> Event {
        let mut e = event(id, Vec::new());
        e.people = people;
        e
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

    #[test]
    fn title_change_emits_event_updated() {
        let prev = vec![event_with_title("e1", "Old Title")];
        let curr = vec![event_with_title("e1", "New Title")];
        let records = detect_changes(&prev, &curr, now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::EventUpdated);
        assert_eq!(records[0].event_id.0, "e1");
    }

    #[test]
    fn unchanged_title_emits_no_event_updated() {
        let prev = vec![event_with_title("e1", "Same Title")];
        let curr = vec![event_with_title("e1", "Same Title")];
        let records = detect_changes(&prev, &curr, now());
        assert!(records.is_empty());
    }

    #[test]
    fn new_talk_emits_schedule_added() {
        let prev = vec![event_with_talks("e1", vec![])];
        let curr = vec![event_with_talks("e1", vec![talk("t1", None)])];
        let records = detect_changes(&prev, &curr, now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::ScheduleAdded);
        assert_eq!(records[0].detail.as_deref(), Some("t1"));
    }

    #[test]
    fn existing_talk_emits_no_schedule_added() {
        let prev = vec![event_with_talks("e1", vec![talk("t1", None)])];
        let curr = vec![event_with_talks("e1", vec![talk("t1", None)])];
        let records = detect_changes(&prev, &curr, now());
        assert!(records.is_empty());
    }

    #[test]
    fn new_event_speaker_emits_speaker_added() {
        let prev = vec![event_with_people("e1", vec![])];
        let curr = vec![event_with_people("e1", vec![speaker("Terence Tao")])];
        let records = detect_changes(&prev, &curr, now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::SpeakerAdded);
        assert_eq!(records[0].detail.as_deref(), Some("Terence Tao"));
    }

    #[test]
    fn new_talk_speaker_emits_speaker_added() {
        let prev = vec![event_with_talks("e1", vec![talk("t1", None)])];
        let curr = vec![event_with_talks("e1", vec![talk("t1", Some("Don Zagier"))])];
        let records = detect_changes(&prev, &curr, now());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::SpeakerAdded);
        assert_eq!(records[0].detail.as_deref(), Some("Don Zagier"));
    }

    #[test]
    fn existing_speaker_emits_no_speaker_added() {
        let prev = vec![event_with_people("e1", vec![speaker("Terence Tao")])];
        let curr = vec![event_with_people("e1", vec![speaker("Terence Tao")])];
        let records = detect_changes(&prev, &curr, now());
        assert!(records.is_empty());
    }

    #[test]
    fn non_speaker_role_emits_no_speaker_added() {
        let mut organizer = speaker("Alice Organizer");
        organizer.role = PersonRole::Organizer;
        let prev = vec![event_with_people("e1", vec![])];
        let curr = vec![event_with_people("e1", vec![organizer])];
        let records = detect_changes(&prev, &curr, now());
        assert!(
            records.iter().all(|r| r.kind != ChangeKind::SpeakerAdded),
            "non-Speaker role must not emit SpeakerAdded, got {records:?}"
        );
    }

    #[test]
    fn multiple_changes_one_event_sorted() {
        let prev = vec![event_with_title("e1", "Old")];
        let mut curr = event_with_title("e1", "New");
        curr.media.push(video("https://youtube.com/v/1"));
        let curr = vec![curr];
        let records = detect_changes(&prev, &curr, now());
        let kinds: Vec<_> = records.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ChangeKind::EventUpdated));
        assert!(kinds.contains(&ChangeKind::MediaAdded));
    }

    // T2-4: same start/end/timezone but different `original_text`/`precision`
    // are parser artifacts, not a semantic change — must not emit
    // `event_updated` (§47 false-positive risk).
    #[test]
    fn date_artifact_change_emits_no_event_updated() {
        let mut prev = event("e1", vec![]);
        prev.date = EventDate {
            start: Some(radar_core::DateTimeOrDate::Date(
                NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            )),
            end: None,
            timezone: None,
            original_text: "Sep 1".into(),
            precision: radar_core::DatePrecision::Day,
        };
        let mut curr = event("e1", vec![]);
        curr.date = EventDate {
            start: Some(radar_core::DateTimeOrDate::Date(
                NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            )),
            end: None,
            timezone: None,
            original_text: "September 1st, 2026".into(),
            precision: radar_core::DatePrecision::Range,
        };
        let records = detect_changes(
            std::slice::from_ref(&prev),
            std::slice::from_ref(&curr),
            now(),
        );
        assert!(
            records.is_empty(),
            "date artifact change (original_text/precision) must not emit event_updated: {records:?}"
        );
    }

    // T2-4 negative: an actual start-date change MUST still emit event_updated.
    #[test]
    fn date_start_change_emits_event_updated() {
        let mut prev = event("e1", vec![]);
        prev.date = EventDate {
            start: Some(radar_core::DateTimeOrDate::Date(
                NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            )),
            end: None,
            timezone: None,
            original_text: "Sep 1".into(),
            precision: radar_core::DatePrecision::Day,
        };
        let mut curr = event("e1", vec![]);
        curr.date = EventDate {
            start: Some(radar_core::DateTimeOrDate::Date(
                NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
            )),
            end: None,
            timezone: None,
            original_text: "Sep 1".into(),
            precision: radar_core::DatePrecision::Day,
        };
        let records = detect_changes(
            std::slice::from_ref(&prev),
            std::slice::from_ref(&curr),
            now(),
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ChangeKind::EventUpdated);
    }
}
