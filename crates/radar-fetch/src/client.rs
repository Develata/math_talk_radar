//! HTTP client (§15, §16). The async fetch path lands in M2; this establishes
//! the builder, UA, and the `FetchedDocument` constructor adapters depend on.
use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use radar_core::FetchedDocument;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;

use crate::policy::HttpPolicy;

#[derive(Debug, thiserror::Error)]
pub enum FetchBuildError {
    #[error("HTTP client build failed: {0}")]
    Build(#[from] reqwest::Error),
    #[error("invalid HTTP policy: {0}")]
    InvalidPolicy(String),
}

/// HTTP fetcher bound to an [`HttpPolicy`]. The real client (concurrency,
/// timeout, retry, robots, global deadline, body cap) lands in M2.
#[derive(Debug, Clone)]
pub struct FetchClient {
    policy: HttpPolicy,
    inner: reqwest::Client,
    /// Per-host concurrency semaphores (FS-2). Shared across all clones so
    /// every `fetch_one` call — entrypoint or enrichment — charges the
    /// correct host, not the source entrypoint host.
    host_sems: Arc<tokio::sync::Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl FetchClient {
    pub fn new(policy: HttpPolicy) -> Result<Self, FetchBuildError> {
        if policy.global_concurrency == 0 {
            return Err(FetchBuildError::InvalidPolicy(
                "global_concurrency must be >= 1 (0 would hang the semaphore forever)".into(),
            ));
        }
        if policy.per_host_concurrency == 0 {
            return Err(FetchBuildError::InvalidPolicy(
                "per_host_concurrency must be >= 1 (0 would hang the semaphore forever)".into(),
            ));
        }
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
        Ok(Self {
            policy,
            inner,
            host_sems: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        })
    }

    pub fn policy(&self) -> HttpPolicy {
        self.policy
    }

    pub fn handle(&self) -> &reqwest::Client {
        &self.inner
    }

    /// Acquire a per-host concurrency permit for the host of `url` (FS-2).
    /// The permit is released when the returned guard is dropped.
    pub(crate) async fn acquire_host_permit(&self, url: &Url) -> Option<OwnedSemaphorePermit> {
        let host = url.host_str().unwrap_or("").to_string();
        let sem = {
            let mut map = self.host_sems.lock().await;
            map.entry(host)
                .or_insert_with(|| Arc::new(Semaphore::new(self.policy.per_host_concurrency)))
                .clone()
        };
        sem.acquire_owned().await.ok()
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
