//! Date and UTC instant representations shared by public JSON and redb values.

use std::fmt;

use jiff::{Timestamp, civil::Date, tz::Offset};
use schemars::{
    JsonSchema,
    r#gen::SchemaGenerator,
    schema::{InstanceType, Schema, SchemaObject},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Schema proxy for a UTC instant, including when nested in an Option.
pub struct TimestampSchema;

impl JsonSchema for TimestampSchema {
    fn is_referenceable() -> bool {
        false
    }

    fn schema_name() -> String {
        "DateTime".to_owned()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        formatted_string("date-time")
    }
}

pub(super) fn date_schema(_: &mut SchemaGenerator) -> Schema {
    formatted_string("date")
}

fn formatted_string(format: &str) -> Schema {
    SchemaObject {
        instance_type: Some(InstanceType::String.into()),
        format: Some(format.to_owned()),
        ..Default::default()
    }
    .into()
}

struct CalendarDate(Date);

impl fmt::Display for CalendarDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let year = self.0.year();
        let month = self.0.month();
        let day = self.0.day();
        if year < 0 {
            write!(f, "{year:+05}-{month:02}-{day:02}")
        } else {
            write!(f, "{year:04}-{month:02}-{day:02}")
        }
    }
}

pub(super) fn serialize_date<S: Serializer>(date: &Date, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&CalendarDate(*date))
}

pub(super) fn deserialize_date<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Date, D::Error> {
    let value = String::deserialize(deserializer)?;
    Date::strptime("%Y-%m-%d", &value).map_err(de::Error::custom)
}

/// Render UTC with the requested fractional width and the established ISO year spelling.
/// Ordered redb keys use nine digits. JSON values use the legacy 0/3/6/9 groups.
pub fn format_utc(timestamp: Timestamp, precision: usize) -> impl fmt::Display {
    fmt::from_fn(move |f| {
        let utc = Offset::UTC.to_datetime(timestamp);
        write!(
            f,
            "{}T{:.precision$}Z",
            CalendarDate(utc.date()),
            utc.time()
        )
    })
}

pub fn serialize_timestamp<S: Serializer>(
    timestamp: &Timestamp,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let nanos = timestamp.subsec_nanosecond();
    let precision = if nanos == 0 {
        0
    } else if nanos % 1_000_000 == 0 {
        3
    } else if nanos % 1_000 == 0 {
        6
    } else {
        9
    };
    serializer.collect_str(&format_utc(*timestamp, precision))
}

pub fn deserialize_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Timestamp, D::Error> {
    let value = String::deserialize(deserializer)?;
    parse_timestamp(&value).map_err(de::Error::custom)
}

pub fn parse_timestamp(value: &str) -> Result<Timestamp, jiff::Error> {
    // The prior wire format uses a four-digit magnitude for negative years.
    // Jiff's temporal parser requires six digits when a year sign is present.
    if let Some(year_and_rest) = value.strip_prefix('-')
        && year_and_rest.as_bytes().get(4) == Some(&b'-')
    {
        return format!("-00{year_and_rest}").parse();
    }
    value.parse()
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct WireTimestamp(
    #[serde(
        serialize_with = "serialize_timestamp",
        deserialize_with = "deserialize_timestamp"
    )]
    pub Timestamp,
);

pub mod optional_timestamp {
    use super::{Deserialize, Deserializer, Serialize, Serializer, Timestamp, WireTimestamp};

    pub fn serialize<S: Serializer>(
        timestamp: &Option<Timestamp>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        timestamp.map(WireTimestamp).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Timestamp>, D::Error> {
        Option::<WireTimestamp>::deserialize(deserializer)
            .map(|value| value.map(|wrapped| wrapped.0))
    }
}

#[cfg(test)]
mod tests {
    use crate::DateTimeOrDate;

    #[test]
    fn legacy_date_and_instant_strings_round_trip() {
        for value in [
            "2026-11-17",
            "-0001-01-01",
            "-0001-01-01T00:00:00Z",
            "1969-12-31T16:30:00.120Z",
            "2026-11-17T16:30:00Z",
            "2026-11-17T16:30:00.120Z",
            "2026-11-17T16:30:00.120340Z",
            "2026-11-17T16:30:00.120340560Z",
        ] {
            let json = format!(r#""{value}""#);
            let parsed: DateTimeOrDate = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
        }
    }

    #[test]
    fn offset_instant_keeps_its_utc_date_and_time() {
        let value: DateTimeOrDate = serde_json::from_str(r#""2026-11-18T00:30:00+08:00""#).unwrap();
        assert!(matches!(value, DateTimeOrDate::DateTime(_)));
        assert_eq!(crate::date::calendar_date(&value).to_string(), "2026-11-17");
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#""2026-11-17T16:30:00Z""#
        );
        assert!(serde_json::from_str::<DateTimeOrDate>(r#""2026-11-18T00:30:00""#).is_err());
    }
}
