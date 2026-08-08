//! Source adapter contract (§13).
//!
//! Parsers must NOT perform network I/O. All fetching is done by the
//! coordinator + `radar-fetch`, which hands prepared [`FetchedDocument`]s to
//! these methods. Implementations live in `radar-adapters`.
use thiserror::Error;
use url::Url;

use crate::config::SourceSpec;
use crate::date::EventDate;
use crate::document::{FetchPlan, FetchedDocument};
use crate::model::{Event, SourceEvidence};

/// A partial event discovered from a list/feed page, to be enriched later.
#[derive(Debug, Clone)]
pub struct EventStub {
    pub title: String,
    pub url: Url,
    pub date_hint: Option<EventDate>,
    pub source: SourceEvidence,
}

/// The output of enrichment: a (partially) filled [`Event`] plus the stub it
/// came from. The coordinator normalizes, matches, dedups, and ranks it.
#[derive(Debug, Clone)]
pub struct EventCandidate {
    pub event: Event,
    pub stub: EventStub,
}

#[derive(Debug, Error)]
pub enum AdapterError {
    // thiserror reserves a field named `source` for `Error::source`; this
    // field must stay named `source_id`.
    #[error("parse error in source {source_id}: {message}")]
    Parse { source_id: String, message: String },
    #[error("dynamic/JS-rendered source unsupported: {0}")]
    DynamicUnsupported(String),
    #[error("budget exhausted for source {0}")]
    BudgetExhausted(String),
}

pub trait SourceAdapter: Send + Sync {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError>;

    fn plan_enrichment(&self, event: &EventStub, source: &SourceSpec) -> Vec<FetchPlan>;

    fn enrich(
        &self,
        event: EventStub,
        documents: &[FetchedDocument],
        source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError>;
}
