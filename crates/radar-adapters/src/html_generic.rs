//! Generic HTML fallback adapter (§P-5 priority #6, last resort).
//!
//! Per §74, the generic `<a>` parser must never be the primary strategy; it is
//! only used when no structured or configured adapter applies.
use radar_core::{
    AdapterError, EventCandidate, EventStub, FetchPlan, FetchedDocument, SourceAdapter, SourceSpec,
};

use crate::{empty_discover, empty_plan, not_implemented_enrich};

#[derive(Debug, Default)]
pub struct HtmlGenericAdapter;

impl SourceAdapter for HtmlGenericAdapter {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        empty_discover(document, source)
    }

    fn plan_enrichment(&self, event: &EventStub, source: &SourceSpec) -> Vec<FetchPlan> {
        empty_plan(event, source)
    }

    fn enrich(
        &self,
        event: EventStub,
        documents: &[FetchedDocument],
        source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        let _ = documents;
        not_implemented_enrich(event, source)
    }
}
