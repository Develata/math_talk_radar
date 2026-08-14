//! Scan-mode + date-window filtering (§27.2).
//!
//! Pure domain logic: deciding whether an event belongs in a scan result set
//! given the user's mode (upcoming / recordings / both) and date window. Lives
//! in `radar-core` so it is testable without the CLI composition root and can
//! be reused by any downstream consumer of the domain model.
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::Event;
use crate::date::interval_overlap;

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
}

/// §27.2 window + mode filter. Uses `interval_overlap` (§8) so multi-day events
/// that span into the window are correctly included — a conference starting
/// before the window but ending inside it is still relevant.
///
/// Mode semantics:
/// - `upcoming`: event must have a parseable date overlapping the window.
///   Media is irrelevant; undated events are dropped (can't confirm upcoming).
/// - `recordings`: event must have ≥1 media. If it also has a date, the date
///   must overlap the window. Undated media events are kept (can't window-filter
///   what we can't date).
/// - `both`: the union of the two branches — a dated event in the window passes
///   regardless of media; an undated event passes only if it has media.
pub fn matches_mode_and_window(
    event: &Event,
    mode: ScanMode,
    today: NaiveDate,
    before_days: u32,
    after_days: u32,
) -> bool {
    let has_media = !event.media.is_empty();
    let has_start = event.date.start.is_some();

    let window_start = today
        .checked_sub_signed(chrono::Duration::days(before_days as i64))
        .unwrap_or(NaiveDate::MIN);
    let window_end = today
        .checked_add_signed(chrono::Duration::days(after_days as i64))
        .unwrap_or(NaiveDate::MAX);
    let in_window = interval_overlap(&event.date, window_start, window_end);

    match mode {
        ScanMode::Recordings => has_media && (!has_start || in_window),
        ScanMode::Upcoming => has_start && in_window,
        ScanMode::Both => (has_start && in_window) || (has_media && !has_start),
    }
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
        event_with_range(start, None, media)
    }

    fn event_with_range(
        start: Option<chrono::NaiveDate>,
        end: Option<chrono::NaiveDate>,
        media: Vec<MediaResource>,
    ) -> Event {
        Event {
            id: EventId("e".to_string()),
            title: "T".to_string(),
            url: None,
            event_type: EventType::Conference,
            status: EventStatus::Unknown,
            date: EventDate {
                start: start.map(crate::date::DateTimeOrDate::Date),
                end: end.map(crate::date::DateTimeOrDate::Date),
                timezone: None,
                original_text: String::new(),
                precision: start.map_or(DatePrecision::Unknown, |_| {
                    if end.is_some() {
                        DatePrecision::Range
                    } else {
                        DatePrecision::Day
                    }
                }),
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
    fn both_mode_no_date_no_media_dropped() {
        // HIGH-1 regression: undated, no-media events must NOT pass the both
        // filter. The previous implementation returned `mode == ScanMode::Both`
        // for missing start dates, which kept historical noise in the output.
        let ev = event_with(None, Vec::new());
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Both,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn both_mode_dated_media_outside_window_dropped() {
        // HIGH-1 regression: a dated event with media that is OUTSIDE the
        // window must be dropped in both mode. The previous implementation
        // short-circuited on `has_media` before checking the window.
        let ev = event_with(Some(date(2025, 1, 1)), vec![media()]);
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Both,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn both_mode_dated_no_media_outside_window_dropped() {
        let ev = event_with(Some(date(2025, 1, 1)), Vec::new());
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Both,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn h1_multiday_event_spanning_into_window_passes() {
        // H1 regression: a conference starting before the window but ending
        // inside it overlaps the window. The previous start-date-only check
        // dropped it because start < window_start.
        let ev = event_with_range(Some(date(2025, 12, 20)), Some(date(2026, 1, 5)), Vec::new());
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn h1_multiday_event_ending_before_window_dropped() {
        let ev = event_with_range(Some(date(2025, 11, 1)), Some(date(2025, 11, 5)), Vec::new());
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn h1_recordings_mode_multiday_in_window_passes() {
        let ev = event_with_range(
            Some(date(2025, 12, 20)),
            Some(date(2026, 1, 5)),
            vec![media()],
        );
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Recordings,
            date(2026, 1, 1),
            30,
            30
        ));
    }

    #[test]
    fn h1_recordings_mode_multiday_outside_window_dropped() {
        let ev = event_with_range(
            Some(date(2025, 11, 1)),
            Some(date(2025, 11, 5)),
            vec![media()],
        );
        assert!(!matches_mode_and_window(
            &ev,
            ScanMode::Recordings,
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

    // CORE-16: very large before_days/after_days must not panic (checked
    // arithmetic). An overflow on one side means the window is unbounded on
    // that side, so the event passes if it satisfies the other bound.
    #[test]
    fn large_window_does_not_panic() {
        let ev = event_with(Some(date(2026, 1, 15)), Vec::new());
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            u32::MAX,
            30
        ));
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            30,
            u32::MAX
        ));
        assert!(matches_mode_and_window(
            &ev,
            ScanMode::Upcoming,
            date(2026, 1, 1),
            u32::MAX,
            u32::MAX
        ));
    }
}
