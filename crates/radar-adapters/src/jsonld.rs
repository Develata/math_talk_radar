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
use scraper::Html;
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
        let mut global_idx = 0usize;
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
                    .or_else(|| {
                        ev.get("@id")
                            .and_then(|v| v.as_str())
                            .and_then(|s| Url::parse(s).ok())
                    })
                    .unwrap_or_else(|| {
                        // ADAP-12: query param (not fragment) so canonicalize_url
                        // preserves it, keeping each unnamed event's id distinct.
                        // ADAP-16: counter is global across all JSON-LD blocks so
                        // unnamed events in different blocks get distinct ids.
                        // H04: base the synthetic URL on final_url (post-redirect)
                        // so the mtr-eid query param attaches to the real origin.
                        let mut u = document.final_url.clone();
                        u.query_pairs_mut()
                            .append_pair("mtr-eid", &global_idx.to_string());
                        u
                    });
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
                        source_url: document.final_url.clone(),
                        evidence: None,
                        captured_at: Some(document.fetched_at),
                        native_id: None,
                    },
                });
                global_idx += 1;
            }
        }
        Ok(stubs)
    }

    fn plan_enrichment(&self, event: &EventStub, _source: &SourceSpec) -> Vec<FetchPlan> {
        // When a JSON-LD Event had no `url`, `discover` synthesized the stub's
        // url from the listing page (`document.final_url`) with a synthetic
        // `mtr-eid` query param to keep distinct unnamed events from collapsing
        // to one `event_id`. Re-fetching that same listing page once per
        // url-less stub would burn the request budget N times for data already
        // obtained during discover. Emit no fetch; `enrich` then builds a
        // minimal event from the stub fields (title, date_hint).
        if urls_match_ignoring_mtr_eid(&event.url, &event.source.source_url) {
            return Vec::new();
        }
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
                    if !event_matches_stub(ev, &stub) {
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
                        source_url: doc.final_url.clone(),
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

/// Compare two URLs ignoring only the synthetic `mtr-eid` query param that
/// `discover` adds to url-less JSON-LD events (ADAP-12).
fn urls_match_ignoring_mtr_eid(a: &Url, b: &Url) -> bool {
    if a == b {
        return true;
    }
    let strip = |u: &Url| -> Url {
        let mut s = u.clone();
        let kept: Vec<(String, String)> = s
            .query_pairs()
            .filter(|(k, _)| k != "mtr-eid")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        s.query_pairs_mut().clear();
        s.query_pairs_mut()
            .extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        s
    };
    strip(a) == strip(b)
}

/// ADAP-21: match a JSON-LD event node to a stub by url/@id OR name.
/// H05: url/@id is checked FIRST — it is a stable identity signal. Title
/// alone is fragile: a detail page listing multiple events with the same
/// name (e.g. a series of "Seminar" talks) would let the first matching
/// node claim every same-named stub, contaminating description/location
/// enrichment across distinct events. Identity-based matching wins when
/// available; name is the fallback.
fn event_matches_stub(ev: &serde_json::Value, stub: &EventStub) -> bool {
    let ev_url = ev
        .get("url")
        .and_then(|v| v.as_str())
        .and_then(|s| Url::parse(s).ok());
    if ev_url.as_ref().is_some_and(|u| u == &stub.url) {
        return true;
    }
    let ev_id = ev
        .get("@id")
        .and_then(|v| v.as_str())
        .and_then(|s| Url::parse(s).ok());
    if ev_id.as_ref().is_some_and(|u| u == &stub.url) {
        return true;
    }
    // R9-H05: when the event carries a parseable url or @id that did NOT
    // match the stub, falling back to name would cross-contaminate
    // same-named events on the same page. Name fallback is safe only when
    // the event has no parseable url/@id at all.
    if ev_url.is_some() || ev_id.is_some() {
        return false;
    }
    ev.get("name").and_then(|v| v.as_str()) == Some(&stub.title)
}

/// Parse every `<script type="application/ld+json">` block in `html` into a
/// [`serde_json::Value`]. Malformed blocks are silently skipped (§66: parsers
/// must not panic on untrusted input).
fn extract_jsonld_blocks(html: &str) -> Vec<serde_json::Value> {
    if html.is_empty() {
        return Vec::new();
    }
    let fragment = Html::parse_fragment(html);
    let Some(selector) = helpers::cached_selector(r#"script[type="application/ld+json"]"#) else {
        return Vec::new();
    };
    fragment
        .select(selector)
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
/// Person object, an array of Person objects, an array of strings, or a bare
/// string.
fn extract_person_names(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(obj) => obj
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
        serde_json::Value::Array(arr) => arr.iter().flat_map(extract_person_names).collect(),
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
    fn discover_unnamed_events_get_distinct_ids() {
        let html = r#"<script type="application/ld+json">[
            {"@type":"Event","startDate":"2026-08-01"},
            {"@type":"Event","startDate":"2026-09-01"}
        ]</script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        assert_eq!(stubs.len(), 2);
        assert_eq!(stubs[0].title, "Untitled");
        assert_eq!(stubs[1].title, "Untitled");
        assert_ne!(
            stubs[0].url, stubs[1].url,
            "unnamed events on the same listing page must get distinct synthetic urls"
        );
        assert_ne!(
            event_id(&stubs[0].title, stubs[0].url.as_str()),
            event_id(&stubs[1].title, stubs[1].url.as_str()),
            "unnamed events must produce distinct event_ids"
        );
    }

    #[test]
    fn discover_unnamed_event_uses_at_id_when_url_absent() {
        let html = r#"<script type="application/ld+json">
            {"@type":"Event","name":"Has @id","@id":"https://example.com/event/x"}
        </script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].url.as_str(), "https://example.com/event/x");
    }

    // ADAP-16: the per-block enumerate index used to reset to 0 for each
    // JSON-LD block, so two unnamed events in separate blocks both got
    // mtr-eid=0 → same event_id → second event silently lost. The counter
    // must be global across all blocks.
    #[test]
    fn discover_unnamed_events_across_blocks_get_distinct_ids() {
        let html = r#"<script type="application/ld+json">
            {"@type":"Event","startDate":"2026-08-01"}
        </script>
        <script type="application/ld+json">
            {"@type":"Event","startDate":"2026-09-01"}
        </script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stubs = JsonLdAdapter.discover(&doc, &spec).unwrap();
        assert_eq!(stubs.len(), 2);
        assert_ne!(
            stubs[0].url, stubs[1].url,
            "unnamed events in separate blocks must get distinct synthetic urls"
        );
        assert_ne!(
            event_id(&stubs[0].title, stubs[0].url.as_str()),
            event_id(&stubs[1].title, stubs[1].url.as_str()),
            "unnamed events in separate blocks must get distinct event_ids"
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
    fn enrich_extracts_performer_array_of_strings() {
        let html = r#"<script type="application/ld+json">
        {"@type":"Event","name":"Talk B","url":"https://example.com/b",
         "performer":["Alice","Bob"]}</script>"#;
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
        assert_eq!(candidate.event.talks.len(), 1);
        assert_eq!(candidate.event.talks[0].speaker.len(), 2);
        assert_eq!(candidate.event.talks[0].speaker[0].canonical_name, "Alice");
        assert_eq!(candidate.event.talks[0].speaker[1].canonical_name, "Bob");
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

    // ADAP-21: enrich must match by url or @id, not just by name. A detail
    // page may use a slightly different title than the listing feed; matching
    // only on name would lose the performer/description enrichment.
    #[test]
    fn enrich_matches_by_url_when_name_differs() {
        let html = r#"<script type="application/ld+json">
        {"@type":"Event","name":"Slightly Different Title","url":"https://example.com/a",
         "performer":{"name":"Prof. X"},
         "description":"Real description"}
        </script>"#;
        let doc = make_doc(html);
        let spec = make_spec();
        let stub = EventStub {
            title: "Original Title".to_string(),
            url: Url::parse("https://example.com/a").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-jsonld".to_string(),
                source_url: Url::parse("https://example.com/page").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let candidate = JsonLdAdapter
            .enrich(stub, std::slice::from_ref(&doc), &spec)
            .unwrap();
        assert_eq!(
            candidate.event.description.as_deref(),
            Some("Real description"),
            "enrich must match by url even when the JSON-LD event name differs from the stub title"
        );
        assert!(!candidate.event.talks.is_empty());
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

    // R9-H05: two same-named events on one page must not cross-contaminate.
    // Before the fix, event_matches_stub checked name first, so the first
    // JSON-LD node matched every same-named stub — enriching stub B with
    // node A's description. With url-first matching, each stub claims only
    // its own node.
    #[test]
    fn enrich_same_name_events_matched_by_url_not_name() {
        let html = r#"<script type="application/ld+json">
        {"@type":"Event","name":"Seminar","url":"https://example.com/s1",
         "description":"Seminar ONE description"}
        </script>
        <script type="application/ld+json">
        {"@type":"Event","name":"Seminar","url":"https://example.com/s2",
         "description":"Seminar TWO description"}
        </script>"#;
        let doc = make_doc(html);
        let spec = make_spec();

        let stub_s2 = EventStub {
            title: "Seminar".to_string(),
            url: Url::parse("https://example.com/s2").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-jsonld".to_string(),
                source_url: Url::parse("https://example.com/page").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let candidate = JsonLdAdapter
            .enrich(stub_s2, std::slice::from_ref(&doc), &spec)
            .unwrap();
        assert_eq!(
            candidate.event.description.as_deref(),
            Some("Seminar TWO description"),
            "stub for s2 must get s2's description, not s1's"
        );
    }
}
