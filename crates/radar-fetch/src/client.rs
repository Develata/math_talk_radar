//! HTTP client (§15, §16). The async fetch path lands in M2; this establishes
//! the builder, UA, and the `FetchedDocument` constructor adapters depend on.
use chrono::{DateTime, Utc};
use radar_core::FetchedDocument;
use url::Url;

use crate::policy::HttpPolicy;

#[derive(Debug, thiserror::Error)]
pub enum FetchBuildError {
    #[error("HTTP client build failed: {0}")]
    Build(#[from] reqwest::Error),
}

/// HTTP fetcher bound to an [`HttpPolicy`]. The real client (concurrency,
/// timeout, retry, robots, global deadline, body cap) lands in M2.
#[derive(Debug, Clone)]
pub struct FetchClient {
    policy: HttpPolicy,
    inner: reqwest::Client,
}

impl FetchClient {
    pub fn new(policy: HttpPolicy) -> Result<Self, FetchBuildError> {
        let inner = reqwest::Client::builder()
            .timeout(policy.request_timeout)
            .connect_timeout(policy.connect_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                "math_talk_radar/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/Develata/math_talk_radar)"
            ))
            .build()?;
        Ok(Self { policy, inner })
    }

    pub fn policy(&self) -> HttpPolicy {
        self.policy
    }

    pub fn handle(&self) -> &reqwest::Client {
        &self.inner
    }
}

/// Construct a `FetchedDocument` from raw response parts. Used by the async
/// fetch path (M2) and by offline fixture loaders in tests.
pub fn make_document(
    url: Url,
    final_url: Url,
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
    fetched_at: DateTime<Utc>,
) -> FetchedDocument {
    FetchedDocument {
        url,
        final_url,
        status,
        content_type,
        body,
        fetched_at,
    }
}
