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
