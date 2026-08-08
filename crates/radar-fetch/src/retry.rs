//! Retry decision logic (§15). Pure.
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
/// `retry_after` is forwarded for 429 but the caller is responsible for not
/// breaching the global scan deadline.
pub fn retry_for_status(status: u16, retry_after: Option<Duration>) -> RetryDecision {
    match status {
        408 | 429 => RetryDecision::Retry { after: retry_after },
        500..=599 => RetryDecision::Retry { after: None },
        _ => RetryDecision::NoRetry,
    }
}

#[cfg(test)]
mod tests {
    use super::{RetryDecision, retry_for_status};

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
}
