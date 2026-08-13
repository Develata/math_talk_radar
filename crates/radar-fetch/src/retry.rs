//! Retry decision logic (§15). Pure.
use chrono::{DateTime, NaiveDateTime, Utc};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Retry, optionally after sleeping (e.g. honoring `Retry-After`).
    Retry {
        after: Option<Duration>,
    },
    NoRetry,
}

/// Classify an HTTP status for retry. Per §15: retry 408, 429, 5xx only.
/// `retry_after` is forwarded for 429 and 5xx (many servers send `Retry-After`
/// with 503/502/504) but the caller is responsible for not breaching the
/// global scan deadline.
pub fn retry_for_status(status: u16, retry_after: Option<Duration>) -> RetryDecision {
    match status {
        408 | 429 => RetryDecision::Retry { after: retry_after },
        500..=599 => RetryDecision::Retry { after: retry_after },
        _ => RetryDecision::NoRetry,
    }
}

/// Classify a `reqwest` network error for retry. Per §15: retry connection
/// reset and transient network failure. Never retry timeout (handled by the
/// scan deadline), redirect errors, or body/decode errors.
pub fn is_transient_network_error(err: &reqwest::Error) -> bool {
    if err.is_timeout() {
        return false;
    }
    if err.is_connect() {
        return true;
    }
    // Walk the error source chain for connection reset / aborted.
    let mut source = std::error::Error::source(err);
    while let Some(e) = source {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted => {
                    return true;
                }
                _ => {}
            }
        }
        source = e.source();
    }
    false
}

/// FETCH-2: Parse a `Retry-After` header value per RFC 7231 §7.1.3.
/// Accepts either delta-seconds (`u64`) or an HTTP-date in IMF-fixdate
/// (`Sun, 06 Nov 1994 08:49:37 GMT`). Returns the delay until the indicated
/// instant, clamped to zero if the date is in the past relative to `now`.
/// `now` is a parameter (not read from the system clock) so the function stays
/// pure and unit-testable.
pub fn parse_retry_after(value: &str, now: DateTime<Utc>) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let target = NaiveDateTime::parse_from_str(trimmed, "%a, %d %b %Y %H:%M:%S GMT").ok()?;
    let target_utc = target.and_utc();
    let secs = target_utc.signed_duration_since(now).num_seconds();
    if secs <= 0 {
        return Some(Duration::ZERO);
    }
    u64::try_from(secs).ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::{Duration, RetryDecision, parse_retry_after, retry_for_status};
    use chrono::{NaiveDateTime, Utc};

    #[test]
    fn retries_transient_only() {
        assert!(matches!(
            retry_for_status(408, None),
            RetryDecision::Retry { .. }
        ));
        assert!(matches!(
            retry_for_status(429, None),
            RetryDecision::Retry { .. }
        ));
        assert!(matches!(
            retry_for_status(500, None),
            RetryDecision::Retry { .. }
        ));
        assert!(matches!(
            retry_for_status(503, None),
            RetryDecision::Retry { .. }
        ));
    }

    #[test]
    fn retry_after_passthrough_5xx() {
        let d = Duration::from_secs(7);
        assert_eq!(
            retry_for_status(503, Some(d)),
            RetryDecision::Retry { after: Some(d) }
        );
        assert_eq!(
            retry_for_status(502, Some(d)),
            RetryDecision::Retry { after: Some(d) }
        );
        assert_eq!(
            retry_for_status(504, Some(d)),
            RetryDecision::Retry { after: Some(d) }
        );
    }

    #[test]
    fn retry_after_passthrough_429() {
        let d = Duration::from_secs(3);
        assert_eq!(
            retry_for_status(429, Some(d)),
            RetryDecision::Retry { after: Some(d) }
        );
    }

    #[test]
    fn retry_after_none_when_absent() {
        assert_eq!(
            retry_for_status(503, None),
            RetryDecision::Retry { after: None }
        );
    }

    #[test]
    fn no_retry_for_terminal() {
        assert!(matches!(
            retry_for_status(400, None),
            RetryDecision::NoRetry
        ));
        assert!(matches!(
            retry_for_status(401, None),
            RetryDecision::NoRetry
        ));
        assert!(matches!(
            retry_for_status(403, None),
            RetryDecision::NoRetry
        ));
        assert!(matches!(
            retry_for_status(404, None),
            RetryDecision::NoRetry
        ));
        assert!(matches!(
            retry_for_status(410, None),
            RetryDecision::NoRetry
        ));
        assert!(matches!(
            retry_for_status(200, None),
            RetryDecision::NoRetry
        ));
    }

    #[test]
    fn parse_retry_after_delta_seconds() {
        let now = Utc::now();
        assert_eq!(
            parse_retry_after("120", now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(parse_retry_after("0", now), Some(Duration::from_secs(0)));
        assert_eq!(
            parse_retry_after("  42  ", now),
            Some(Duration::from_secs(42))
        );
    }

    #[test]
    fn parse_retry_after_http_date_future() {
        let now = NaiveDateTime::parse_from_str(
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "%a, %d %b %Y %H:%M:%S GMT",
        )
        .unwrap()
        .and_utc();
        let future = "Sun, 06 Nov 1994 08:51:37 GMT";
        assert_eq!(
            parse_retry_after(future, now),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn parse_retry_after_http_date_past_clamps_to_zero() {
        let now = NaiveDateTime::parse_from_str(
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "%a, %d %b %Y %H:%M:%S GMT",
        )
        .unwrap()
        .and_utc();
        let past = "Sun, 06 Nov 1994 08:47:37 GMT";
        assert_eq!(parse_retry_after(past, now), Some(Duration::from_secs(0)));
    }

    #[test]
    fn parse_retry_after_invalid_returns_none() {
        let now = Utc::now();
        assert_eq!(parse_retry_after("not a date", now), None);
        assert_eq!(parse_retry_after("", now), None);
    }
}
