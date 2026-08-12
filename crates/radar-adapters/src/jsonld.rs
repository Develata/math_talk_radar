//! JSON-LD Event adapter (§P-5 structured-source priority #4).
//!
//! Extracts schema.org Event data from `<script type="application/ld+json">`
//! blocks. `discover` maps top-level Event nodes (and `@graph` entries) to
//! [`EventStub`]s; `enrich` re-parses the detail page's JSON-LD to fill the
//! [`Event`]. TALK-001: `performer` and `subEvent` performer names are promoted
//! to [`Talk`] structs with [`PersonRole::Speaker`] (structured-source
//! evidence, §P-2 / §6.2 — a name in a structured field may yield Speaker,
//! unlike a name in body text or a title).
use radar_core::date::parse_date;
use radar_core::{
    AccessInfo, AdapterError, Event, EventCandidate, EventDate, EventStatus, EventStub, FetchPlan,
    FetchedDocument, Location, OnlineAvailability, PersonHit, PersonRole, PublicAccess,
    ScoreComponents, SourceAdapter, SourceEvidence, SourceSpec, Talk, TalkId, deterministic_id,
    event_id,
};
use scraper::{Html, Selector};
use url::Url;

use crate::helpers;

#[derive(Debug, Default)]
pub struct JsonLdAdapter;

impl SourceAdapter for JsonLdAdapter {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        let html = crate::helpers::doc_body(&document.body);
        let mut stubs = Vec::new();
        for block in extract_jsonld_blocks(&html) {
            for ev in find_events(&block).into_iter().flatten() {
                let title = ev
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Untitled")
                    .to_string();
                let url = ev
                    .get("url")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Url::parse(s).ok())
                    .unwrap_or_else(|| document.url.clone());
                let date_hint = ev
                    .get("startDate")
                    .and_then(|v| v.as_str())
                    .map(|s| parse_date(s).unwrap_or_else(|_| EventDate::unknown(s.to_string())));
                stubs.push(EventStub {
                    title,
                    url,
                    date_hint,
                    source: SourceEvidence {
                        source_id: source.id.clone(),
                        source_url: document.url.clone(),
                        evidence: None,
                        captured_at: Some(document.fetched_at),
                        native_id: None,
                    },
                });
            }
        }
        Ok(stubs)
    }

    fn plan_enrichment(&self, event: &EventStub, _source: &SourceSpec) -> Vec<FetchPlan> {
        vec![FetchPlan {
            url: event.url.clone(),
            depth: 1,
            reason: "jsonld_detail".into(),
        }]
    }

    fn enrich(
        &self,
        stub: EventStub,
        documents: &[FetchedDocument],
        source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        let mut talks = Vec::new();
        let mut description = None;
        let mut location = None;
        let mut access = PublicAccess::Unknown;

        if let Some(doc) = documents.first() {
            let html = crate::helpers::doc_body(&doc.body);
            let document = scraper::Html::parse_document(&html);
            access = helpers::classify_access(&document);
            for block in extract_jsonld_blocks(&html) {
                for ev in find_events(&block).into_iter().flatten() {
                    if ev.get("name").and_then(|v| v.as_str()) != Some(&stub.title) {
                        continue;
                    }
                    if description.is_none() {
                        description = ev
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                    if location.is_none() {
                        location = ev.get("location").and_then(extract_location);
                    }
                    let talk_source = SourceEvidence {
                        source_id: source.id.clone(),
                        source_url: doc.url.clone(),
                        evidence: None,
                        captured_at: Some(doc.fetched_at),
                        native_id: None,
                    };
                    // TALK-001: performer → one Talk with all performers as
                    // co-speakers (schema.org co-presenters).
                    if let Some(performers) = ev.get("performer") {
                        let names = extract_person_names(performers);
                        if !names.is_empty() {
                            talks.push(make_talk(
                                &stub.title,
                                &names,
                                talk_source.clone(),
                                "jsonld:performer",
                            ));
                        }
                    }
                    // TALK-001: subEvent → one Talk per sub-event, speakers from
                    // its own performer field (schema.org conference→talk model).
                    if let Some(sub_events) = ev.get("subEvent") {
                        for sub_ev in iter_subevents(sub_events) {
                            let sub_title = sub_ev
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&stub.title)
                                .to_string();
                            if let Some(performers) = sub_ev.get("performer") {
                                let names = extract_person_names(performers);
                                if !names.is_empty() {
                                    talks.push(make_talk(
                                        &sub_title,
                                        &names,
                                        talk_source.clone(),
                                        "jsonld:subEvent:performer",
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        let date = stub
            .date_hint
            .clone()
            .unwrap_or_else(|| EventDate::unknown(String::new()));

        let event = Event {
            id: event_id(&stub.title, stub.url.as_str()),
            title: stub.title.clone(),
            url: Some(stub.url.clone()),
            event_type: helpers::detect_event_type(&stub.title),
            status: EventStatus::Unknown,
            date,
            location,
            description,
            topics: Vec::new(),
            people: Vec::new(),
            talks,
            media: Vec::new(),
            access: AccessInfo {
                access,
                online: OnlineAvailability::Unknown,
            },
            sources: vec![stub.source.clone()],
            score: 0.0,
            score_components: ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        };

        Ok(EventCandidate { event, stub })
    }
}

// --- helpers ----------------------------------------------------------------

/// Parse every `<script type="application/ld+json">` block in `html` into a
/// [`serde_json::Value`]. Malformed blocks are silently skipped (§66: parsers
/// must not panic on untrusted input).
fn extract_jsonld_blocks(html: &str) -> Vec<serde_json::Value> {
    if html.is_empty() {
        return Vec::new();
    }
    let fragment = Html::parse_fragment(html);
    let Ok(selector) = Selector::parse(r#"script[type="application/ld+json"]"#) else {
        return Vec::new();
    };
    fragment
        .select(&selector)
        .filter_map(|el| {
            let text = el.text().collect::<String>();
            serde_json::from_str::<serde_json::Value>(&text).ok()
        })
        .collect()
}

/// Recursively collect references to JSON nodes whose `@type` contains `Event`.
/// Handles a single Event object, an array of nodes, and `@graph` containers.
fn find_events(value: &serde_json::Value) -> Option<Vec<&serde_json::Value>> {
    if is_event_type(value) {
        return Some(vec![value]);
    }
    match value {
        serde_json::Value::Array(arr) => {
            let events: Vec<&serde_json::Value> = arr.iter().filter(|v| is_event_type(v)).collect();
            if events.is_empty() {
                None
            } else {
                Some(events)
            }
        }
        serde_json::Value::Object(obj) => obj.get("@graph").and_then(find_events),
        _ => None,
    }
}

fn is_event_type(value: &serde_json::Value) -> bool {
    let Some(t) = value.get("@type") else {
        return false;
    };
    match t {
        serde_json::Value::String(s) => s.contains("Event"),
        serde_json::Value::Array(arr) => arr
            .iter()
            .any(|v| v.as_str().is_some_and(|s| s.contains("Event"))),
        _ => false,
    }
}

/// Extract person names from a schema.org `performer`-style value: a single
/// Person object, an array of Person objects, or a bare string.
fn extract_person_names(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(obj) => obj
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| {
                v.get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
            .collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Normalize a `subEvent` value into a list of sub-event nodes.
fn iter_subevents(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    match value {
        serde_json::Value::Array(arr) => arr.iter().collect(),
        v if is_event_type(v) => vec![v],
        _ => Vec::new(),
    }
}

/// Build a [`Location`] from a schema.org `location` value: either a Place
/// object with a `name`, or a bare string.
fn extract_location(value: &serde_json::Value) -> Option<Location> {
    let name_str = match value {
        serde_json::Value::Object(obj) => obj.get("name").and_then(|n| n.as_str()),
        serde_json::Value::String(s) => Some(s.as_str()),
        _ => None,
    };
    let name = name_str?;
    Some(Location {
        name: name.to_string(),
        city: None,
        country: None,
        venue: None,
    })
}

fn make_talk(
    title: &str,
    names: &[String],
    source_evidence: SourceEvidence,
    evidence_tag: &str,
) -> Talk {
    let speaker = names
        .iter()
        .map(|name| PersonHit {
            canonical_name: name.clone(),
            matched_text: name.clone(),
            role: PersonRole::Speaker,
            evidence: Some(evidence_tag.to_string()),
            confidence: 1.0,
            scholar_tags: Vec::new(),
        })
        .collect::<Vec<_>>();
    Talk {
        id: TalkId(deterministic_id(&[title, &names.join(", ")])),
        title: title.to_string(),
        speaker,
        date_time: None,
        abstract_text: None,
        topics: Vec::new(),
        media: Vec::new(),
        source: source_evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radar_core::{AdapterKind, SourceKind, SourceTier};

    fn make_spec() -> SourceSpec {
        SourceSpec {
            id: "test-jsonld".to_string(),
            name: "Test JSON-LD".to_string(),
            tier: SourceTier::default(),
            kind: SourceKind::default(),
            adapter: AdapterKind::default(),
            entrypoint: None,
            allowed_hosts: Vec::new(),
            max_depth: 2,
            request_budget: 20,
            media_strategy: None,
            dynamic: false,
            enabled: false,
            fixture: None,
            selectors: None,
        }
    }

    fn make_doc(html: &str) -> FetchedDocument {
        FetchedDocument {
            url: Url::parse("https://example.com/page").unwrap(),
            final_url: Url::parse("https://example.com/page").unwrap(),
            status: 200,
            content_type: Some("text/html".into()),
            body: html.as_bytes().to_vec(),
            // chrono is not a direct dependency of radar-adapters; rely on the
            // `From<SystemTime> for DateTime<Utc>` impl exposed transitively.
            fetched_at: std::time::SystemTime::now().into(),
        }
    }

    #[test]
    fn discover_finds_events() {
        let html = r#"<script type="application/ld+json">[
            {"@type":"Event","name":"Conf A","url":"https://example.com/a","startDate":"2026-08-01"},
            {"@type":"Event","name":"Conf B","url":"https://example.com/b","startDate":"2026-09-01"},
            {"@type":"Event","name":"Conf C","url":"https://example.com/c","startDate":"2026-10-01"}
        ]</script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        assert!(
            stubs.len() >= 3,
            "expected at least 3 stubs, got {}",
            stubs.len()
        );
        assert_eq!(stubs[0].title, "Conf A");
        assert_eq!(stubs[1].title, "Conf B");
        assert_eq!(stubs[2].title, "Conf C");
        assert!(
            stubs[0].date_hint.is_some(),
            "startDate should yield a date hint"
        );
    }

    #[test]
    fn discover_no_jsonld_returns_empty() {
        let html = "<html><body>no jsonld</body></html>";
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        assert!(stubs.is_empty());
    }

    #[test]
    fn discover_malformed_block_skipped() {
        let html = r#"<script type="application/ld+json">not json</script>
        <script type="application/ld+json">{"@type":"Event","name":"OK","url":"https://example.com/ok"}</script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].title, "OK");
    }

    #[test]
    fn discover_graph_container() {
        let html = r#"<script type="application/ld+json">
        {"@context":"https://schema.org","@graph":[
            {"@type":"Event","name":"Graph A","url":"https://example.com/ga"},
            {"@type":"Organization","name":"NotAnEvent"}
        ]}</script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].title, "Graph A");
    }

    #[test]
    fn enrich_extracts_performer() {
        let html = r#"<script type="application/ld+json">
        {"@type":"Event","name":"Talk A","url":"https://example.com/a",
         "performer":{"name":"Prof. X"}}</script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        let candidate = JsonLdAdapter
            .enrich(
                stubs.into_iter().next().unwrap(),
                std::slice::from_ref(&doc),
                &spec,
            )
            .unwrap();
        assert!(!candidate.event.talks.is_empty());
        assert_eq!(
            candidate.event.talks[0].speaker[0].role,
            PersonRole::Speaker
        );
        assert_eq!(
            candidate.event.talks[0].speaker[0].canonical_name,
            "Prof. X"
        );
        assert_eq!(candidate.stub.title, "Talk A");
    }

    #[test]
    fn enrich_extracts_subevent_performers() {
        let html = r#"<script type="application/ld+json">
        {"@type":"Event","name":"Conf X","url":"https://example.com/x",
         "subEvent":[
           {"@type":"Event","name":"Talk One","performer":{"name":"Alice"}},
           {"@type":"Event","name":"Talk Two","performer":[{"name":"Bob"},{"name":"Cy"}]}
         ]}</script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        let candidate = JsonLdAdapter
            .enrich(
                stubs.into_iter().next().unwrap(),
                std::slice::from_ref(&doc),
                &spec,
            )
            .unwrap();
        assert_eq!(candidate.event.talks.len(), 2);
        assert_eq!(candidate.event.talks[0].title, "Talk One");
        assert_eq!(candidate.event.talks[0].speaker.len(), 1);
        assert_eq!(candidate.event.talks[0].speaker[0].canonical_name, "Alice");
        assert_eq!(candidate.event.talks[1].title, "Talk Two");
        assert_eq!(candidate.event.talks[1].speaker.len(), 2);
        assert_eq!(candidate.event.talks[1].speaker[0].canonical_name, "Bob");
        assert_eq!(candidate.event.talks[1].speaker[1].canonical_name, "Cy");
    }

    #[test]
    fn enrich_extracts_description_and_location() {
        let html = r#"<script type="application/ld+json">
        {"@type":"Event","name":"Conf Y","url":"https://example.com/y",
         "description":"A conference on algebra",
         "location":{"name":"Berlin"}}</script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        let candidate = JsonLdAdapter
            .enrich(
                stubs.into_iter().next().unwrap(),
                std::slice::from_ref(&doc),
                &spec,
            )
            .unwrap();
        assert_eq!(
            candidate.event.description.as_deref(),
            Some("A conference on algebra")
        );
        assert_eq!(candidate.event.location.as_ref().unwrap().name, "Berlin");
    }

    #[test]
    fn enrich_without_documents_is_minimal() {
        let stub = EventStub {
            title: "Orphan Talk".to_string(),
            url: Url::parse("https://example.com/o").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-jsonld".to_string(),
                source_url: Url::parse("https://example.com/page").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let spec = make_spec();
        let candidate = JsonLdAdapter.enrich(stub, &[], &spec).unwrap();
        assert_eq!(candidate.event.title, "Orphan Talk");
        assert!(candidate.event.talks.is_empty());
        assert!(candidate.event.description.is_none());
        assert!(candidate.event.location.is_none());
        assert_eq!(candidate.event.sources.len(), 1);
    }
}
