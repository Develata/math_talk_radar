//! Date model and parsing entry points (§8).
//!
//! The full date parser (range formats, cross-month ranges, US vs UK ordering,
//! interval-overlap filtering) lands in M1. This module establishes the
//! canonical [`EventDate`] shape and the clock-injection surface used by tests.
use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventDate {
    pub start: Option<DateTimeOrDate>,
    pub end: Option<DateTimeOrDate>,
    pub timezone: Option<String>,
    pub original_text: String,
    pub precision: DatePrecision,
}

impl EventDate {
    /// Best-effort calendar-date extraction of the start. Returns `None` when
    /// `start` is absent. Used by dedup signatures (§25) and interval overlap
    /// (§8) which compare on the date component regardless of time.
    pub fn start_date(&self) -> Option<NaiveDate> {
        self.start.as_ref().map(to_naive_date)
    }
}

/// A date that may be a calendar date or an exact instant. Serialized
/// untagged so consumers can branch on shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DateTimeOrDate {
    DateTime(DateTime<Utc>),
    Date(NaiveDate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatePrecision {
    Year,
    Month,
    Day,
    DateTime,
    Range,
    Unknown,
}

/// A wall-clock range for an individual talk (§5.3 `date_time`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DateTimeRange {
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub timezone: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DateError {
    // Currently unreachable: unparseable text returns Unknown instead (M1 design choice).
    #[error("unparseable date text: {0}")]
    Unparseable(String),
}

/// Parse an IANA timezone name. Returns `None` if unknown. Used by the CLI's
/// `--timezone` and `--today` clock-injection surface (§8.4).
pub fn parse_timezone(name: &str) -> Option<chrono_tz::Tz> {
    name.parse::<chrono_tz::Tz>().ok()
}

// --- M1 date parsing -------------------------------------------------------
//
// The parser is deterministic and clock-free: no year inference, no timezone
// extraction. Unparseable text is retained as `precision = Unknown` rather than
// rejected, so downstream ranking can demote but never silently drop (§8).

/// Map an English month name (full or 3-letter abbreviation, case-insensitive)
/// to its 1-based month number. Returns `None` for unknown names.
fn month_from_name(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "january" | "jan" => Some(1),
        "february" | "feb" => Some(2),
        "march" | "mar" => Some(3),
        "april" | "apr" => Some(4),
        "may" => Some(5),
        "june" | "jun" => Some(6),
        "july" | "jul" => Some(7),
        "august" | "aug" => Some(8),
        "september" | "sep" | "sept" => Some(9),
        "october" | "oct" => Some(10),
        "november" | "nov" => Some(11),
        "december" | "dec" => Some(12),
        _ => None,
    }
}

// Compile-time invariant: each pattern below is a statically verified regex
// literal. `Regex::new` cannot fail for these strings; the `expect` calls rely
// on that invariant and are safe under `#![forbid(unsafe_code)]`.
fn re_same_month_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})\s*[–-]\s*(\d{1,2})\s+([A-Za-z]+)\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_cross_month_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})\s+([A-Za-z]+)\s*[–-]\s*(\d{1,2})\s+([A-Za-z]+)\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_us_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([A-Za-z]+)\s+(\d{1,2})\s*[–-]\s*(\d{1,2}),?\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_us_single() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([A-Za-z]+)\s+(\d{1,2}),?\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_day_month_year_single() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})\s+([A-Za-z]+)\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

/// Parse a free-text date string into an [`EventDate`].
///
/// Always returns `Ok`; unparseable text (including year-less dates) produces
/// an `EventDate` with `precision = Unknown`. The crate forbids clocks, so no
/// year inference is performed. `timezone` is always `None` here — timezone
/// handling is a CLI concern (M4).
pub fn parse_date(text: &str) -> Result<EventDate, DateError> {
    let trimmed = text.trim();

    // 1. ISO 8601 single date: "2026-08-08".
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(EventDate {
            start: Some(DateTimeOrDate::Date(d)),
            end: None,
            timezone: None,
            original_text: text.to_string(),
            precision: DatePrecision::Day,
        });
    }

    // 2-6. Range / US / DMY patterns, tried in order; first match wins.
    //      Invalid dates (e.g. Feb 30) fall through to Unknown via `?`.
    if let Some(ed) = try_same_month_range(trimmed, text)
        .or_else(|| try_cross_month_range(trimmed, text))
        .or_else(|| try_us_range(trimmed, text))
        .or_else(|| try_us_single(trimmed, text))
        .or_else(|| try_day_month_year_single(trimmed, text))
    {
        return Ok(ed);
    }

    Ok(EventDate {
        start: None,
        end: None,
        timezone: None,
        original_text: text.to_string(),
        precision: DatePrecision::Unknown,
    })
}

/// Same-month range: "3–7 August 2026" / "3-7 August 2026".
fn try_same_month_range(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_same_month_range().captures(trimmed)?;
    let d1: u32 = caps[1].parse().ok()?;
    let d2: u32 = caps[2].parse().ok()?;
    let month = month_from_name(&caps[3])?;
    let year: i32 = caps[4].parse().ok()?;
    let start = NaiveDate::from_ymd_opt(year, month, d1)?;
    let end = NaiveDate::from_ymd_opt(year, month, d2)?;
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

/// Cross-month range: "31 August – 4 September 2026".
fn try_cross_month_range(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_cross_month_range().captures(trimmed)?;
    let d1: u32 = caps[1].parse().ok()?;
    let m1 = month_from_name(&caps[2])?;
    let d2: u32 = caps[3].parse().ok()?;
    let m2 = month_from_name(&caps[4])?;
    let year: i32 = caps[5].parse().ok()?;
    let start = NaiveDate::from_ymd_opt(year, m1, d1)?;
    let end = NaiveDate::from_ymd_opt(year, m2, d2)?;
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

/// US format range: "August 3–7, 2026" / "August 3-7, 2026".
fn try_us_range(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_us_range().captures(trimmed)?;
    let month = month_from_name(&caps[1])?;
    let d1: u32 = caps[2].parse().ok()?;
    let d2: u32 = caps[3].parse().ok()?;
    let year: i32 = caps[4].parse().ok()?;
    let start = NaiveDate::from_ymd_opt(year, month, d1)?;
    let end = NaiveDate::from_ymd_opt(year, month, d2)?;
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

/// US format single: "August 3, 2026".
fn try_us_single(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_us_single().captures(trimmed)?;
    let month = month_from_name(&caps[1])?;
    let day: u32 = caps[2].parse().ok()?;
    let year: i32 = caps[3].parse().ok()?;
    let d = NaiveDate::from_ymd_opt(year, month, day)?;
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(d)),
        end: None,
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Day,
    })
}

/// Day Month Year single: "8 August 2026".
fn try_day_month_year_single(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_day_month_year_single().captures(trimmed)?;
    let day: u32 = caps[1].parse().ok()?;
    let month = month_from_name(&caps[2])?;
    let year: i32 = caps[3].parse().ok()?;
    let d = NaiveDate::from_ymd_opt(year, month, day)?;
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(d)),
        end: None,
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Day,
    })
}

/// Extract the [`NaiveDate`] component from a [`DateTimeOrDate`].
fn to_naive_date(d: &DateTimeOrDate) -> NaiveDate {
    match d {
        DateTimeOrDate::DateTime(dt) => dt.date_naive(),
        DateTimeOrDate::Date(d) => *d,
    }
}

/// Interval-overlap predicate (§8): `event.start <= query.end AND
/// event.end >= query.start`. `None` start/end are treated as unbounded. A
/// single-day event (`Some` start, `None` end) overlaps iff its start lies
/// within `[query_start, query_end]`.
pub fn interval_overlap(event: &EventDate, query_start: NaiveDate, query_end: NaiveDate) -> bool {
    let event_start_inclusive = match &event.start {
        None => true,
        Some(d) => to_naive_date(d) <= query_end,
    };
    let event_end_inclusive = match &event.end {
        None => match &event.start {
            None => true,
            Some(sd) => to_naive_date(sd) >= query_start,
        },
        Some(d) => to_naive_date(d) >= query_start,
    };
    event_start_inclusive && event_end_inclusive
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn start_date(ed: &EventDate) -> NaiveDate {
        match ed.start.as_ref().unwrap() {
            DateTimeOrDate::Date(n) => *n,
            DateTimeOrDate::DateTime(_) => panic!("expected Date, got DateTime"),
        }
    }

    fn end_date(ed: &EventDate) -> NaiveDate {
        match ed.end.as_ref().unwrap() {
            DateTimeOrDate::Date(n) => *n,
            DateTimeOrDate::DateTime(_) => panic!("expected Date, got DateTime"),
        }
    }

    // DATE-001: same-month range (en-dash and ASCII hyphen).
    #[test]
    fn date_001_same_month_range() {
        let ed = parse_date("3–7 August 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 3));
        assert_eq!(end_date(&ed), d(2026, 8, 7));
        assert_eq!(ed.timezone, None);
        assert_eq!(ed.original_text, "3–7 August 2026");

        let ed2 = parse_date("3-7 August 2026").unwrap();
        assert_eq!(ed2.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed2), d(2026, 8, 3));
        assert_eq!(end_date(&ed2), d(2026, 8, 7));
    }

    // DATE-002: cross-month range.
    #[test]
    fn date_002_cross_month_range() {
        let ed = parse_date("31 August – 4 September 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 31));
        assert_eq!(end_date(&ed), d(2026, 9, 4));
        assert_eq!(ed.original_text, "31 August – 4 September 2026");
    }

    // DATE-003: US format range and single.
    #[test]
    fn date_003_us_format() {
        let ed = parse_date("August 3–7, 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 3));
        assert_eq!(end_date(&ed), d(2026, 8, 7));

        let single = parse_date("August 3, 2026").unwrap();
        assert_eq!(single.precision, DatePrecision::Day);
        assert_eq!(start_date(&single), d(2026, 8, 3));
        assert!(single.end.is_none());
    }

    // DATE-004: interval-overlap filtering.
    #[test]
    fn date_004_interval_overlap() {
        let q_start = d(2026, 8, 10);
        let q_end = d(2026, 8, 20);

        let mk = |s: Option<NaiveDate>, e: Option<NaiveDate>, p: DatePrecision| EventDate {
            start: s.map(DateTimeOrDate::Date),
            end: e.map(DateTimeOrDate::Date),
            timezone: None,
            original_text: String::new(),
            precision: p,
        };

        // event fully before query → false
        assert!(!interval_overlap(
            &mk(
                Some(d(2026, 7, 1)),
                Some(d(2026, 7, 5)),
                DatePrecision::Range
            ),
            q_start,
            q_end,
        ));
        // event fully after query → false
        assert!(!interval_overlap(
            &mk(
                Some(d(2026, 9, 1)),
                Some(d(2026, 9, 5)),
                DatePrecision::Range
            ),
            q_start,
            q_end,
        ));
        // event overlapping query (partial) → true
        assert!(interval_overlap(
            &mk(
                Some(d(2026, 8, 15)),
                Some(d(2026, 8, 25)),
                DatePrecision::Range
            ),
            q_start,
            q_end,
        ));
        // event containing query → true
        assert!(interval_overlap(
            &mk(
                Some(d(2026, 8, 1)),
                Some(d(2026, 8, 31)),
                DatePrecision::Range
            ),
            q_start,
            q_end,
        ));
        // event contained by query → true
        assert!(interval_overlap(
            &mk(
                Some(d(2026, 8, 12)),
                Some(d(2026, 8, 18)),
                DatePrecision::Range
            ),
            q_start,
            q_end,
        ));
        // None start (unbounded past), end inside query → true
        assert!(interval_overlap(
            &mk(None, Some(d(2026, 8, 15)), DatePrecision::Range),
            q_start,
            q_end,
        ));
        // None start, end before query → false
        assert!(!interval_overlap(
            &mk(None, Some(d(2026, 7, 1)), DatePrecision::Range),
            q_start,
            q_end,
        ));
        // single-day (Some start, None end) inside query → true
        assert!(interval_overlap(
            &mk(Some(d(2026, 8, 15)), None, DatePrecision::Day),
            q_start,
            q_end,
        ));
        // single-day before query → false
        assert!(!interval_overlap(
            &mk(Some(d(2026, 7, 1)), None, DatePrecision::Day),
            q_start,
            q_end,
        ));
        // both None (Unknown) → true (covers everything)
        assert!(interval_overlap(
            &mk(None, None, DatePrecision::Unknown),
            q_start,
            q_end,
        ));
    }

    // DATE-005: unparsed text retained with precision=Unknown, original preserved.
    #[test]
    fn date_005_unparsed_retained() {
        let ed = parse_date("not a date at all").unwrap();
        assert!(ed.start.is_none());
        assert!(ed.end.is_none());
        assert_eq!(ed.precision, DatePrecision::Unknown);
        assert_eq!(ed.original_text, "not a date at all");

        let yearless = parse_date("3–7 August").unwrap();
        assert!(yearless.start.is_none());
        assert!(yearless.end.is_none());
        assert_eq!(yearless.precision, DatePrecision::Unknown);
        assert_eq!(yearless.original_text, "3–7 August");
    }

    // ISO single date.
    #[test]
    fn parse_iso_single() {
        let ed = parse_date("2026-08-08").unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2026, 8, 8));
        assert!(ed.end.is_none());
    }

    // Day Month Year single.
    #[test]
    fn parse_dmy_single() {
        let ed = parse_date("8 August 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2026, 8, 8));
        assert!(ed.end.is_none());
    }

    // Month abbreviation (US single).
    #[test]
    fn parse_month_abbrev() {
        let ed = parse_date("Aug 3, 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2026, 8, 3));
        assert!(ed.end.is_none());
    }

    // Leading/trailing whitespace trimmed for parsing, preserved in original_text.
    #[test]
    fn parse_trims_whitespace() {
        let ed = parse_date("  2026-08-08  ").unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2026, 8, 8));
        assert_eq!(ed.original_text, "  2026-08-08  ");
    }

    // Invalid calendar date (Feb 30) falls through to Unknown — no inference.
    #[test]
    fn parse_invalid_date_falls_through() {
        let ed = parse_date("30 February 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Unknown);
        assert!(ed.start.is_none());
    }

    // parse_date always returns Ok (never Err), per M1 design.
    #[test]
    fn parse_date_never_errors() {
        let cases = ["", "   ", "2026", "August", "13–14", "garbage!@#"];
        for c in cases {
            assert!(parse_date(c).is_ok(), "parse_date({c:?}) should be Ok");
        }
    }
}
