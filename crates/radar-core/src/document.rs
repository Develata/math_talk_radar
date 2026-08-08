//! Fetched-document and fetch-plan types (§13).
//!
//! These live in `radar-core` (not `radar-fetch`) so the fetch→adapter boundary
//! does not create a crate cycle: adapters must not depend on `radar-fetch`
//! (§11), so the shared document type they both touch is defined here. The fetch
//! layer produces [`FetchedDocument`]; adapters and the coordinator consume it.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone)]
pub struct FetchedDocument {
    pub url: Url,
    pub final_url: Url,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub fetched_at: DateTime<Utc>,
}

/// A planned detail request emitted by
/// [`crate::adapter::SourceAdapter::plan_enrichment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchPlan {
    pub url: Url,
    pub depth: u8,
    pub reason: String,
}
