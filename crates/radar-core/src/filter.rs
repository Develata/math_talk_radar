//! Scan-mode + date-window filtering (§27.2).
//!
//! Pure domain logic: deciding whether an event belongs in a scan result set
//! given the user's mode (upcoming / recordings / both) and date window. Lives
//! in `radar-core` so it is testable without the CLI composition root and can
//! be reused by any downstream consumer of the domain model.
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    Upcoming,
    Recordings,
    Both,
}

impl ScanMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanMode::Upcoming => "upcoming",
            ScanMode::Recordings => "recordings",
            ScanMode::Both => "both",
        }
    }

    fn wants_recordings(self) -> bool {
        matches!(self, ScanMode::Recordings | ScanMode::Both)
    }

    fn wants_upcoming(self) -> bool {
        matches!(self, ScanMode::Upcoming | ScanMode::Both)
    }
}

/// §27.2 window + mode filter. An event passes when:
/// - mode is `recordings` or `both` AND the event has ≥1 media, OR
/// - mode is `upcoming` or `both` AND the event's start date falls within
///   `[today - before_days, today + after_days]`.
///
/// Events with no parseable start date are kept only in `recordings` mode
/// (if they have media) or `both` mode; in `upcoming` mode they are dropped
/// because we cannot confirm they are upcoming.
pub fn matches_mode_and_window(
    event: &Event,
    mode: ScanMode,
    today: NaiveDate,
    before_days: u32,
    after_days: u32,
) -> bool {
    let has_media = !event.media.is_empty();

    if mode.wants_recordings() && has_media {
        return true;
    }

    if !mode.wants_upcoming() {
        return false;
    }

    let Some(start) = event.date.start_date() else {
        return mode == ScanMode::Both;
    };
    let window_start = today - chrono::Duration::days(before_days as i64);
    let window_end = today + chrono::Duration::days(after_days as i64);
    start >= window_start && start <= window_end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::{DatePrecision, EventDate};
    use crate::model::{
        AccessInfo, EventId, EventStatus, EventType, MediaResource, OnlineAvailability,
        PublicAccess, SourceEvidence,
    };
    use url::Url;

    fn event_with(start: Option<chrono::NaiveDate>, media: Vec<MediaResource>) -> Event {
        Event {
            id: EventId("e".to_string()),
            title: "T".to_string(),
            url: None,
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
            media,
            access: AccessInfo {
                access: PublicAccess::Unknown,
                online: OnlineAvailability::Unknown,
            },
            sources: vec![SourceEvidence {
                source_id: "s".to_string(),
                source_url: Url::parse("https://x.com").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            }],
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

    fn media() -> MediaResource {
        MediaResource {
            id: crate::model::MediaId("m".to_string()),
            media_type: crate::model::MediaType::Video,
            title: None,
            url: Url::parse("https://x.com/v").unwrap(),
            platform: None,
            public_access: PublicAccess::Open,
            published_at: None,
            source: SourceEvidence {
                source_id: "s".to_string(),
                source_url: Url::parse("https://x.com").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        }
    }

    #[test]
    fn recordings_mode_keeps_media_without_date() {
        let ev = event_with(None, vec![media()]);
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Recordings,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn upcoming_mode_drops_no_date_even_with_media() {
        let ev = event_with(None, vec![media()]);
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn both_mode_keeps_no_date_via_recordings_branch() {
        let ev = event_with(None, vec![media()]);
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Both,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn both_mode_no_date_no_media_kept_by_explicit_check() {
        let ev = event_with(None, Vec::new());
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Both,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn upcoming_within_window_passes() {
        let ev = event_with(Some(date(2026, 1, 15)), Vec::new());
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn upcoming_outside_window_dropped() {
        let ev = event_with(Some(date(2026, 6, 1)), Vec::new());
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn recordings_mode_drops_no_media_with_date_in_window() {
        let ev = event_with(Some(date(2026, 1, 15)), Vec::new());
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Recordings,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn scan_mode_as_str() {
        assert_eq!(ScanMode::Upcoming.as_str(), "upcoming");
        assert_eq!(ScanMode::Recordings.as_str(), "recordings");
        assert_eq!(ScanMode::Both.as_str(), "both");
    }
}
