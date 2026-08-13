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

    /// An `EventDate` with no known start/end/timezone and `Unknown` precision,
    /// preserving `original_text` for display. Used by adapters when a date
    /// field is absent or unparseable.
    pub fn unknown(original_text: impl Into<String>) -> Self {
        Self {
            start: None,
            end: None,
            timezone: None,
            original_text: original_text.into(),
            precision: DatePrecision::Unknown,
        }
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
        Regex::new(r"^(\d{1,2})(?:st|nd|rd|th)?\s*[–-]\s*(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_cross_month_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)\s*[–-]\s*(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_us_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?\s*[–-]\s*(\d{1,2})(?:st|nd|rd|th)?,?\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_us_single() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_day_month_year_single() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)\s+(\d{4})$")
            .expect("statically verified regex literal")
    })
}

fn re_us_full_date_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(\d{4})\s*(?:[–-]|to)\s*([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(\d{4})$",
        )
        .expect("statically verified regex literal")
    })
}

/// CORE-14: DMY full date range with explicit years on both endpoints:
/// "31 December 2026 - 2 January 2027". The existing `re_cross_month_range`
/// captures only one trailing year and applies it to both endpoints, so
/// cross-year-boundary DMY ranges with explicit years were rejected.
fn re_dmy_full_date_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)\s+(\d{4})\s*(?:[–-]|to)\s*(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)\s+(\d{4})$",
        )
        .expect("statically verified regex literal")
    })
}

fn re_iso_date_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{4}-\d{2}-\d{2})\s*[/–-]\s*(\d{4}-\d{2}-\d{2})$")
            .expect("statically verified regex literal")
    })
}

// Space-separated ISO date range: "2026-08-08 2026-08-10" (no dash/slash).
// Distinct from `re_iso_date_range` which requires an explicit separator; a
// bare space would otherwise be truncated at the first space by the
// single-date path, silently degrading a range to a single Day.
fn re_iso_date_range_space() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{4}-\d{2}-\d{2})\s+(\d{4}-\d{2}-\d{2})$")
            .expect("statically verified regex literal")
    })
}

// --- Year-less variants (used by `parse_date_with_year_hint`) ---------------
//
// Mirrors the five patterns above but without the year capture. Anchored with
// `$` so they do not match the prefix of a year-bearing string (e.g. "Aug 3"
// must not match "August 3, 2026" prematurely).

fn re_same_month_range_no_year() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})(?:st|nd|rd|th)?\s*[–-]\s*(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)$")
            .expect("statically verified regex literal")
    })
}

fn re_cross_month_range_no_year() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)\s*[–-]\s*(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)$")
            .expect("statically verified regex literal")
    })
}

fn re_us_range_no_year() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?\s*[–-]\s*(\d{1,2})(?:st|nd|rd|th)?$")
            .expect("statically verified regex literal")
    })
}

fn re_us_single_no_year() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?$")
            .expect("statically verified regex literal")
    })
}

fn re_dmy_single_no_year() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]+)$")
            .expect("statically verified regex literal")
    })
}

fn re_us_cross_month_range_no_year() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?\s*[–-]\s*([A-Za-z]+)\s+(\d{1,2})(?:st|nd|rd|th)?$")
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
    parse_date_inner(text, None)
}

/// Parse a free-text date string with a year hint for year-less patterns.
///
/// Same as [`parse_date`], but patterns without an explicit year (e.g. "Aug 3",
/// "3–7 August", "Nov 21 – Dec 30") use `year_hint` as the year. For cross-month
/// ranges where the end month precedes the start month (e.g. "Dec 30 – Jan 2"),
/// the end is inferred to `year_hint + 1`.
///
/// The crate remains clock-free: the year is supplied by the caller (typically
/// derived from `FetchedDocument::fetched_at` in `radar-adapters`), not read
/// from a system clock inside `radar-core`.
pub fn parse_date_with_year_hint(text: &str, year_hint: i32) -> Result<EventDate, DateError> {
    parse_date_inner(text, Some(year_hint))
}

fn parse_date_inner(text: &str, year_hint: Option<i32>) -> Result<EventDate, DateError> {
    let trimmed = text.trim();

    // 1. ISO 8601 date range: "2026-08-08 - 2026-08-10" or "2026-08-08/2026-08-10",
    //    including the space-separated form "2026-08-08 2026-08-10".
    if let Some(ed) =
        try_iso_date_range(trimmed, text).or_else(|| try_iso_date_range_space(trimmed, text))
    {
        return Ok(ed);
    }

    // 2. ISO 8601 single date: "2026-08-08" or "2026-08-08T10:00:00".
    //    Time-bearing variants (RFC 3339 / schema.org `datetime` attributes) are
    //    truncated at the first 'T' or space to extract the date component.
    let iso_date_part = trimmed
        .find(['T', ' '])
        .map(|i| &trimmed[..i])
        .unwrap_or(trimmed);
    if let Ok(d) = NaiveDate::parse_from_str(iso_date_part, "%Y-%m-%d") {
        return Ok(EventDate {
            start: Some(DateTimeOrDate::Date(d)),
            end: None,
            timezone: None,
            original_text: text.to_string(),
            precision: DatePrecision::Day,
        });
    }

    // 3-8. Range / US / DMY patterns, tried in order; first match wins.
    //      Invalid dates (e.g. Feb 30) fall through to Unknown via `?`.
    if let Some(ed) = try_us_full_date_range(trimmed, text)
        .or_else(|| try_dmy_full_date_range(trimmed, text))
        .or_else(|| try_same_month_range(trimmed, text))
        .or_else(|| try_cross_month_range(trimmed, text))
        .or_else(|| try_us_range(trimmed, text))
        .or_else(|| try_us_single(trimmed, text))
        .or_else(|| try_day_month_year_single(trimmed, text))
        .or_else(|| year_hint.and_then(|y| try_yearless(trimmed, text, y)))
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

/// ISO 8601 date range: "2026-08-08 - 2026-08-10" or "2026-08-08/2026-08-10".
/// Separator may be en-dash, ASCII hyphen, or slash (ISO 8601 interval).
fn try_iso_date_range(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_iso_date_range().captures(trimmed)?;
    let start = NaiveDate::parse_from_str(&caps[1], "%Y-%m-%d").ok()?;
    let end = NaiveDate::parse_from_str(&caps[2], "%Y-%m-%d").ok()?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

/// Space-separated ISO date range: "2026-08-08 2026-08-10" (no dash/slash).
fn try_iso_date_range_space(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_iso_date_range_space().captures(trimmed)?;
    let start = NaiveDate::parse_from_str(&caps[1], "%Y-%m-%d").ok()?;
    let end = NaiveDate::parse_from_str(&caps[2], "%Y-%m-%d").ok()?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

/// Full US date range: "August 2, 2026 - August 7, 2026". Both endpoints carry
/// explicit year and month; the separator is en-dash or ASCII hyphen.
fn try_us_full_date_range(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_us_full_date_range().captures(trimmed)?;
    let m1 = month_from_name(&caps[1])?;
    let d1: u32 = caps[2].parse().ok()?;
    let y1: i32 = caps[3].parse().ok()?;
    let m2 = month_from_name(&caps[4])?;
    let d2: u32 = caps[5].parse().ok()?;
    let y2: i32 = caps[6].parse().ok()?;
    let start = NaiveDate::from_ymd_opt(y1, m1, d1)?;
    let end = NaiveDate::from_ymd_opt(y2, m2, d2)?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

/// CORE-14: DMY full date range: "31 December 2026 - 2 January 2027".
fn try_dmy_full_date_range(trimmed: &str, original: &str) -> Option<EventDate> {
    let caps = re_dmy_full_date_range().captures(trimmed)?;
    let d1: u32 = caps[1].parse().ok()?;
    let m1 = month_from_name(&caps[2])?;
    let y1: i32 = caps[3].parse().ok()?;
    let d2: u32 = caps[4].parse().ok()?;
    let m2 = month_from_name(&caps[5])?;
    let y2: i32 = caps[6].parse().ok()?;
    let start = NaiveDate::from_ymd_opt(y1, m1, d1)?;
    let end = NaiveDate::from_ymd_opt(y2, m2, d2)?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
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
    if start > end {
        return None;
    }
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
    if start > end {
        return None;
    }
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
    if start > end {
        return None;
    }
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

/// Year-less patterns dispatched under a caller-supplied year hint. Cross-month
/// ranges whose end precedes the start (e.g. "Dec 30 – Jan 2") roll the end
/// forward into `year + 1`; same-month ranges with `d1 > d2` are rejected
/// (no plausible year wrap within one month).
fn try_yearless(trimmed: &str, original: &str, year: i32) -> Option<EventDate> {
    try_same_month_range_no_year(trimmed, original, year)
        .or_else(|| try_cross_month_range_no_year(trimmed, original, year))
        .or_else(|| try_us_cross_month_range_no_year(trimmed, original, year))
        .or_else(|| try_us_range_no_year(trimmed, original, year))
        .or_else(|| try_us_single_no_year(trimmed, original, year))
        .or_else(|| try_dmy_single_no_year(trimmed, original, year))
}

fn try_same_month_range_no_year(trimmed: &str, original: &str, year: i32) -> Option<EventDate> {
    let caps = re_same_month_range_no_year().captures(trimmed)?;
    let d1: u32 = caps[1].parse().ok()?;
    let d2: u32 = caps[2].parse().ok()?;
    let month = month_from_name(&caps[3])?;
    let start = NaiveDate::from_ymd_opt(year, month, d1)?;
    let end = NaiveDate::from_ymd_opt(year, month, d2)?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

fn try_cross_month_range_no_year(trimmed: &str, original: &str, year: i32) -> Option<EventDate> {
    let caps = re_cross_month_range_no_year().captures(trimmed)?;
    let d1: u32 = caps[1].parse().ok()?;
    let m1 = month_from_name(&caps[2])?;
    let d2: u32 = caps[3].parse().ok()?;
    let m2 = month_from_name(&caps[4])?;
    let start = NaiveDate::from_ymd_opt(year, m1, d1)?;
    // If end month precedes start month, the range crosses the year boundary.
    let end_year = if m2 < m1 { year + 1 } else { year };
    let end = NaiveDate::from_ymd_opt(end_year, m2, d2)?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

fn try_us_cross_month_range_no_year(trimmed: &str, original: &str, year: i32) -> Option<EventDate> {
    let caps = re_us_cross_month_range_no_year().captures(trimmed)?;
    let m1 = month_from_name(&caps[1])?;
    let d1: u32 = caps[2].parse().ok()?;
    let m2 = month_from_name(&caps[3])?;
    let d2: u32 = caps[4].parse().ok()?;
    let start = NaiveDate::from_ymd_opt(year, m1, d1)?;
    let end_year = if m2 < m1 { year + 1 } else { year };
    let end = NaiveDate::from_ymd_opt(end_year, m2, d2)?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

fn try_us_range_no_year(trimmed: &str, original: &str, year: i32) -> Option<EventDate> {
    let caps = re_us_range_no_year().captures(trimmed)?;
    let month = month_from_name(&caps[1])?;
    let d1: u32 = caps[2].parse().ok()?;
    let d2: u32 = caps[3].parse().ok()?;
    let start = NaiveDate::from_ymd_opt(year, month, d1)?;
    let end = NaiveDate::from_ymd_opt(year, month, d2)?;
    if start > end {
        return None;
    }
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(start)),
        end: Some(DateTimeOrDate::Date(end)),
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Range,
    })
}

fn try_us_single_no_year(trimmed: &str, original: &str, year: i32) -> Option<EventDate> {
    let caps = re_us_single_no_year().captures(trimmed)?;
    let month = month_from_name(&caps[1])?;
    let day: u32 = caps[2].parse().ok()?;
    let d = NaiveDate::from_ymd_opt(year, month, day)?;
    Some(EventDate {
        start: Some(DateTimeOrDate::Date(d)),
        end: None,
        timezone: None,
        original_text: original.to_string(),
        precision: DatePrecision::Day,
    })
}

fn try_dmy_single_no_year(trimmed: &str, original: &str, year: i32) -> Option<EventDate> {
    let caps = re_dmy_single_no_year().captures(trimmed)?;
    let day: u32 = caps[1].parse().ok()?;
    let month = month_from_name(&caps[2])?;
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

    #[test]
    fn year_hint_us_single() {
        let ed = parse_date_with_year_hint("Nov 21", 2024).unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2024, 11, 21));
        assert!(ed.end.is_none());
        assert_eq!(ed.original_text, "Nov 21");
    }

    #[test]
    fn year_hint_dmy_single() {
        let ed = parse_date_with_year_hint("21 November", 2024).unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2024, 11, 21));
    }

    #[test]
    fn year_hint_same_month_range() {
        let ed = parse_date_with_year_hint("3–7 August", 2026).unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 3));
        assert_eq!(end_date(&ed), d(2026, 8, 7));
    }

    #[test]
    fn year_hint_us_range() {
        let ed = parse_date_with_year_hint("August 3–7", 2026).unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 3));
        assert_eq!(end_date(&ed), d(2026, 8, 7));
    }

    #[test]
    fn year_hint_cross_month_range_rolls_end_year_forward() {
        // "Nov 21 – Dec 30" — same year, no roll.
        let ed = parse_date_with_year_hint("Nov 21 – Dec 30", 2024).unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2024, 11, 21));
        assert_eq!(end_date(&ed), d(2024, 12, 30));

        // "Dec 30 – Jan 02" — end month precedes start, roll end to year+1.
        let ed = parse_date_with_year_hint("Dec 30 – Jan 02", 2024).unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2024, 12, 30));
        assert_eq!(end_date(&ed), d(2025, 1, 2));
    }

    #[test]
    fn year_hint_does_not_override_explicit_year() {
        let ed = parse_date_with_year_hint("August 3, 2026", 2099).unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2026, 8, 3));
    }

    #[test]
    fn year_hint_iso_ignores_hint() {
        let ed = parse_date_with_year_hint("2026-08-08", 2099).unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2026, 8, 8));
    }

    #[test]
    fn year_hint_unparseable_stays_unknown() {
        let ed = parse_date_with_year_hint("not a date", 2024).unwrap();
        assert_eq!(ed.precision, DatePrecision::Unknown);
        assert!(ed.start.is_none());
    }

    #[test]
    fn year_hint_same_month_reversed_rejected() {
        // "7–3 August" — d1 > d2 within same month is not a plausible range.
        let ed = parse_date_with_year_hint("7–3 August", 2026).unwrap();
        assert_eq!(ed.precision, DatePrecision::Unknown);
    }

    #[test]
    fn year_hint_never_errors() {
        let cases = ["", "   ", "2026", "August", "13–14", "garbage!@#"];
        for c in cases {
            assert!(
                parse_date_with_year_hint(c, 2024).is_ok(),
                "parse_date_with_year_hint({c:?}) should be Ok"
            );
        }
    }

    // DATE-006: full US date range "August 2, 2026 - August 7, 2026" (ams-calendar).
    #[test]
    fn date_006_us_full_date_range() {
        let ed = parse_date("August 2, 2026 - August 7, 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 2));
        assert_eq!(end_date(&ed), d(2026, 8, 7));
        assert_eq!(ed.original_text, "August 2, 2026 - August 7, 2026");

        // Cross-month + cross-year variant.
        let ed2 = parse_date("December 30, 2026 - January 2, 2027").unwrap();
        assert_eq!(ed2.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed2), d(2026, 12, 30));
        assert_eq!(end_date(&ed2), d(2027, 1, 2));

        // En-dash separator.
        let ed3 = parse_date("August 2, 2026 – August 7, 2026").unwrap();
        assert_eq!(ed3.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed3), d(2026, 8, 2));
        assert_eq!(end_date(&ed3), d(2026, 8, 7));

        // Same-day range (start == end).
        let ed4 = parse_date("August 19, 2026 - August 19, 2026").unwrap();
        assert_eq!(ed4.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed4), d(2026, 8, 19));
        assert_eq!(end_date(&ed4), d(2026, 8, 19));

        // "to" separator (fields.utoronto.ca series pages).
        let ed5 = parse_date("July 1, 2026 to June 30, 2027").unwrap();
        assert_eq!(ed5.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed5), d(2026, 7, 1));
        assert_eq!(end_date(&ed5), d(2027, 6, 30));
    }

    #[test]
    fn date_007_iso_date_range() {
        let ed = parse_date("2026-08-08 - 2026-08-10").unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 8));
        assert_eq!(end_date(&ed), d(2026, 8, 10));

        let ed2 = parse_date("2026-08-08/2026-08-10").unwrap();
        assert_eq!(ed2.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed2), d(2026, 8, 8));
        assert_eq!(end_date(&ed2), d(2026, 8, 10));

        let ed3 = parse_date("2026-12-30 – 2027-01-02").unwrap();
        assert_eq!(ed3.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed3), d(2026, 12, 30));
        assert_eq!(end_date(&ed3), d(2027, 1, 2));

        let ed4 = parse_date("2026-08-08 - 2026-08-08").unwrap();
        assert_eq!(ed4.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed4), d(2026, 8, 8));
        assert_eq!(end_date(&ed4), d(2026, 8, 8));
    }

    // Space-separated ISO range (no dash/slash) must not silently degrade to a
    // single Day by truncating at the first space.
    #[test]
    fn date_007_iso_date_range_space_separated() {
        let ed = parse_date("2026-08-08 2026-08-10").unwrap();
        assert_eq!(ed.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed), d(2026, 8, 8));
        assert_eq!(end_date(&ed), d(2026, 8, 10));
        assert_eq!(ed.original_text, "2026-08-08 2026-08-10");

        // Cross-year variant.
        let ed2 = parse_date("2026-12-30 2027-01-02").unwrap();
        assert_eq!(ed2.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed2), d(2026, 12, 30));
        assert_eq!(end_date(&ed2), d(2027, 1, 2));

        // Multiple spaces between the two dates.
        let ed3 = parse_date("2026-08-08   2026-08-10").unwrap();
        assert_eq!(ed3.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed3), d(2026, 8, 8));
        assert_eq!(end_date(&ed3), d(2026, 8, 10));

        // A datetime with a space (e.g. "2026-08-08 10:00:00") must NOT be
        // mistaken for a range — it falls through to single-date truncation.
        let ed4 = parse_date("2026-08-08 10:00:00").unwrap();
        assert_eq!(ed4.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed4), d(2026, 8, 8));
        assert!(ed4.end.is_none());
    }

    #[test]
    fn date_008_ordinal_suffixes() {
        let ed = parse_date("August 1st, 2026").unwrap();
        assert_eq!(ed.precision, DatePrecision::Day);
        assert_eq!(start_date(&ed), d(2026, 8, 1));

        let ed2 = parse_date("August 2nd, 2026").unwrap();
        assert_eq!(start_date(&ed2), d(2026, 8, 2));

        let ed3 = parse_date("August 3rd, 2026").unwrap();
        assert_eq!(start_date(&ed3), d(2026, 8, 3));

        let ed4 = parse_date("August 4th, 2026").unwrap();
        assert_eq!(start_date(&ed4), d(2026, 8, 4));

        let ed5 = parse_date("July 1st, 2026 - December 31st, 2026").unwrap();
        assert_eq!(ed5.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed5), d(2026, 7, 1));
        assert_eq!(end_date(&ed5), d(2026, 12, 31));

        let ed6 = parse_date("1st August 2026").unwrap();
        assert_eq!(start_date(&ed6), d(2026, 8, 1));

        let ed7 = parse_date("1st–7th August 2026").unwrap();
        assert_eq!(ed7.precision, DatePrecision::Range);
        assert_eq!(start_date(&ed7), d(2026, 8, 1));
        assert_eq!(end_date(&ed7), d(2026, 8, 7));
    }
}
