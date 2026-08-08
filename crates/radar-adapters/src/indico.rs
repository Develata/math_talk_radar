//! Indico adapter (§P-5 structured-source priority #1, official JSON/API).
use radar_core::{
    AdapterError, EventCandidate, EventStub, FetchPlan, FetchedDocument, SourceAdapter, SourceSpec,
};

use crate::{empty_discover, empty_plan, not_implemented_enrich};

#[derive(Debug, Default)]
pub struct IndicoAdapter;

impl SourceAdapter for IndicoAdapter {
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
