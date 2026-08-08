//! Date model and parsing entry points (§8).
//!
//! The full date parser (range formats, cross-month ranges, US vs UK ordering,
//! interval-overlap filtering) lands in M1. This module establishes the
//! canonical [`EventDate`] shape and the clock-injection surface used by tests.
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventDate {
    pub start: Option<DateTimeOrDate>,
    pub end: Option<DateTimeOrDate>,
    pub timezone: Option<String>,
    pub original_text: String,
    pub precision: DatePrecision,
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
    #[error("unparseable date text: {0}")]
    Unparseable(String),
}

/// Parse an IANA timezone name. Returns `None` if unknown. Used by the CLI's
/// `--timezone` and `--today` clock-injection surface (§8.4).
pub fn parse_timezone(name: &str) -> Option<chrono_tz::Tz> {
    name.parse::<chrono_tz::Tz>().ok()
}
