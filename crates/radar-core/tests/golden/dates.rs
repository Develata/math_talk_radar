//! Date parser golden tests — parses `dates.toml` and evaluates each case
//! against `radar_core::date::parse_date`.

use chrono::NaiveDate;
use radar_core::date::{DatePrecision, DateTimeOrDate, EventDate, parse_date};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Cases {
    cases: Vec<DateCase>,
}

#[derive(Deserialize)]
pub(crate) struct DateCase {
    id: String,
    input: String,
    expected_start: String,
    expected_end: String,
    expected_precision: String,
}

#[derive(Debug, Clone, Copy)]
pub struct DateStats {
    pub total: usize,
    pub passed: usize,
}

fn parse_expected(s: &str) -> Option<NaiveDate> {
    if s == "null" {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn parse_precision(s: &str) -> DatePrecision {
    match s {
        "year" => DatePrecision::Year,
        "month" => DatePrecision::Month,
        "day" => DatePrecision::Day,
        "date_time" => DatePrecision::DateTime,
        "range" => DatePrecision::Range,
        "unknown" => DatePrecision::Unknown,
        _ => DatePrecision::Unknown,
    }
}

fn to_naive(d: &DateTimeOrDate) -> NaiveDate {
    match d {
        DateTimeOrDate::DateTime(dt) => dt.date_naive(),
        DateTimeOrDate::Date(d) => *d,
    }
}

fn event_to_naive(opt: &Option<DateTimeOrDate>) -> Option<NaiveDate> {
    opt.as_ref().map(to_naive)
}

pub fn run(data: &str) -> DateStats {
    let parsed: Cases = toml::from_str(data).expect("dates.toml parses");
    let total = parsed.cases.len();
    let mut passed = 0usize;

    for case in &parsed.cases {
        let ed: EventDate = parse_date(&case.input).expect("parse_date always returns Ok");
        let exp_start = parse_expected(&case.expected_start);
        let exp_end = parse_expected(&case.expected_end);
        let exp_precision = parse_precision(&case.expected_precision);

        let actual_start = event_to_naive(&ed.start);
        let actual_end = event_to_naive(&ed.end);

        if actual_start == exp_start && actual_end == exp_end && ed.precision == exp_precision {
            passed += 1;
        } else {
            eprintln!(
                "FAIL {}: input={:?} expected_start={:?} actual_start={:?} \
                 expected_end={:?} actual_end={:?} expected_precision={:?} \
                 actual_precision={:?}",
                case.id,
                case.input,
                exp_start,
                actual_start,
                exp_end,
                actual_end,
                exp_precision,
                ed.precision
            );
        }
    }

    DateStats { total, passed }
}
