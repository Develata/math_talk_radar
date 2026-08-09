//! Fetch error types (§15).
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("request timed out")]
    Timeout,
    #[error("HTTP error: status {status}")]
    HttpError { status: u16 },
    #[error("response body exceeded {limit} bytes")]
    BodyTooLarge { limit: usize },
    #[error("robots.txt disallows {url}")]
    RobotsDenied { url: Url },
    #[error("redirect to disallowed host")]
    RedirectDisallowed,
    #[error("network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    #[error("budget exhausted")]
    BudgetExhausted,
}

impl FetchError {
    /// Map to SourceStatus (Oracle #6 1:1 mapping).
    pub fn to_source_status(self) -> radar_core::SourceStatus {
        use radar_core::SourceStatus;
        match self {
            FetchError::Timeout => SourceStatus::Timeout,
            FetchError::HttpError { .. } => SourceStatus::HttpError,
            FetchError::BodyTooLarge { .. } => SourceStatus::HttpError,
            FetchError::RobotsDenied { .. } => SourceStatus::RobotsDenied,
            FetchError::RedirectDisallowed => SourceStatus::HttpError,
            FetchError::NetworkError(_) => SourceStatus::HttpError,
            FetchError::BudgetExhausted => SourceStatus::BudgetExhausted,
        }
    }
}
