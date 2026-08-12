//! Indico adapter (§P-5 structured-source priority #1, official JSON/API).
use radar_core::{
    AdapterError, EventCandidate, EventStub, FetchPlan, FetchedDocument, SourceAdapter, SourceSpec,
};

#[derive(Debug, Default)]
pub struct IndicoAdapter;

const NOT_IMPLEMENTED: &str = "Indico adapter not yet implemented; configure the source with a different adapter kind or disable it";

impl SourceAdapter for IndicoAdapter {
    fn discover(
        &self,
        _document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        Err(AdapterError::Parse {
            source_id: source.id.clone(),
            message: NOT_IMPLEMENTED.into(),
        })
    }

    fn plan_enrichment(&self, _event: &EventStub, _source: &SourceSpec) -> Vec<FetchPlan> {
        Vec::new()
    }

    fn enrich(
        &self,
        _event: EventStub,
        _documents: &[FetchedDocument],
        source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        Err(AdapterError::Parse {
            source_id: source.id.clone(),
            message: NOT_IMPLEMENTED.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radar_core::{AdapterKind, SourceKind, SourceTier};
    use url::Url;

    fn make_doc() -> FetchedDocument {
        FetchedDocument {
            url: Url::parse("https://indico.example.com/").unwrap(),
            final_url: Url::parse("https://indico.example.com/").unwrap(),
            status: 200,
            content_type: Some("application/json".into()),
            body: b"{}".to_vec(),
            fetched_at: Default::default(),
        }
    }

    fn make_source() -> SourceSpec {
        SourceSpec {
            id: "test-indico".to_string(),
            name: "Test Indico".to_string(),
            tier: SourceTier::Unknown,
            kind: SourceKind::Indico,
            adapter: AdapterKind::Indico,
            entrypoint: None,
            allowed_hosts: Vec::new(),
            max_depth: 2,
            request_budget: 20,
            media_strategy: None,
            dynamic: false,
            enabled: true,
            fixture: None,
            selectors: None,
        }
    }

    #[test]
    fn discover_returns_error_not_silently_empty() {
        let adapter = IndicoAdapter;
        let doc = make_doc();
        let src = make_source();
        let result = adapter.discover(&doc, &src);
        assert!(
            result.is_err(),
            "discover must surface a clear error, not silently return an empty Vec"
        );
        match result.unwrap_err() {
            AdapterError::Parse { source_id, message } => {
                assert_eq!(source_id, "test-indico");
                assert!(
                    message.contains("Indico"),
                    "message should name Indico: {message}"
                );
            }
            other => panic!("expected AdapterError::Parse, got {other:?}"),
        }
    }

    #[test]
    fn enrich_returns_error_not_silently_empty() {
        let adapter = IndicoAdapter;
        let src = make_source();
        let stub = EventStub {
            title: "e".into(),
            url: Url::parse("https://indico.example.com/e/1").unwrap(),
            date_hint: None,
            source: radar_core::SourceEvidence {
                source_id: src.id.clone(),
                source_url: Url::parse("https://indico.example.com/").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let result = adapter.enrich(stub, &[], &src);
        assert!(result.is_err(), "enrich must surface a clear error");
    }
}
