//! Configured-HTML adapter: source-specific selectors declared in
//! `config/sources.toml` (§P-5 priority #5). Implementation lands in M2/M6.
use radar_core::{
    AdapterError, EventCandidate, EventStub, FetchPlan, FetchedDocument, SourceAdapter, SourceSpec,
};

use crate::{empty_discover, empty_plan, not_implemented_enrich};

#[derive(Debug, Default)]
pub struct HtmlConfigAdapter;

impl SourceAdapter for HtmlConfigAdapter {
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
