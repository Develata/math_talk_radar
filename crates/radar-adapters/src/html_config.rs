//! Configured-HTML adapter (SRC-004, ADR-0005). Parses site-specific event
//! list and detail pages using CSS selectors carried on `SourceSpec::selectors`.
//!
//! Selectors are never hardcoded here: every query reads from `source.selectors`.
//! A configured-HTML source without `selectors` fails fast with `AdapterError`
//! (Metis D5: no automatic fallback to the generic HTML adapter).
use scraper::{ElementRef, Html, Node, Selector};

use chrono::Datelike;

use radar_core::config::HtmlSelectors;
use radar_core::date::{DatePrecision, EventDate, parse_date, parse_date_with_year_hint};
use radar_core::{
    AccessInfo, AdapterError, Event, EventCandidate, EventStatus, EventStub, FetchPlan,
    FetchedDocument, Location, OnlineAvailability, PersonHit, PersonRole, PublicAccess,
    ScoreComponents, SourceAdapter, SourceEvidence, SourceSpec, Talk, TalkId, deterministic_id,
    event_id,
};

use crate::helpers::{classify_access, detect_event_type, detect_media};

#[derive(Debug, Default)]
pub struct HtmlConfigAdapter;

impl SourceAdapter for HtmlConfigAdapter {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        let selectors = require_selectors(source)?;
        let body = crate::helpers::doc_body(&document.body);
        let html = Html::parse_document(&body);
        let list_selector = parse_selector(&source.id, "list", &selectors.list)?;
        let link_selector = parse_selector(&source.id, "list_link", &selectors.list_link)?;
        let title_selector = selectors
            .list_title
            .as_deref()
            .map(|s| parse_selector(&source.id, "list_title", s))
            .transpose()?;
        let date_selector = selectors
            .list_date
            .as_deref()
            .map(|s| parse_selector(&source.id, "list_date", s))
            .transpose()?;

        let mut stubs = Vec::new();
        for container in html.select(&list_selector) {
            for link in container.select(&link_selector) {
                let href = match link.attr("href") {
                    Some(h) => h,
                    None => continue,
                };
                let url = match document.url.join(href) {
                    Ok(u) => u,
                    Err(_) => continue,
                };
                let title = match &title_selector {
                    Some(sel) => match first_text_in(&container, sel) {
                        Some(t) => t,
                        None => continue,
                    },
                    None => crate::helpers::clean_text(&link.text().collect::<String>()),
                };
                if title.is_empty() {
                    continue;
                }
                let date_hint = date_selector
                    .as_ref()
                    .and_then(|sel| first_date_in(&container, sel, document.fetched_at.year()));
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
            reason: "html_config_detail".into(),
        }]
    }

    fn enrich(
        &self,
        event: EventStub,
        documents: &[FetchedDocument],
        source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        let selectors = require_selectors(source)?;

        // No detail document: build a minimal event straight from the stub.
        let Some(doc) = documents.first() else {
            let built = build_minimal_event(&event);
            return Ok(EventCandidate {
                event: built,
                stub: event,
            });
        };

        // Snapshot stub fields before the eventual move into EventCandidate.stub.
        let stub_title = event.title.clone();
        let stub_url = event.url.clone();
        let stub_source = event.source.clone();
        let date_hint = event.date_hint.clone();

        let body = crate::helpers::doc_body(&doc.body);
        let base_url = doc.final_url.clone();
        let html = Html::parse_document(&body);

        let title_selector = parse_selector(&source.id, "detail_title", &selectors.detail_title)?;
        let date_selector = parse_selector(&source.id, "detail_date", &selectors.detail_date)?;

        let title = first_text(&html, &title_selector).unwrap_or_else(|| stub_title.clone());
        let detail_date = first_date(&html, &date_selector);
        let event_date = match detail_date {
            Some(d) => d,
            None => date_hint.unwrap_or_else(|| parse_or_unknown("")),
        };

        let location = optional_first_text(
            &html,
            &source.id,
            "detail_location",
            selectors.detail_location.as_deref(),
        )?
        .map(|name| Location {
            name,
            city: None,
            country: None,
            venue: None,
        });

        let description = optional_first_text(
            &html,
            &source.id,
            "detail_description",
            selectors.detail_description.as_deref(),
        )?;

        let speakers = optional_all_texts(
            &html,
            &source.id,
            "detail_speaker",
            selectors.detail_speaker.as_deref(),
        )?;

        let people: Vec<PersonHit> = speakers.iter().map(|name| speaker_hit(name)).collect();
        let talks: Vec<Talk> = speakers
            .iter()
            .map(|name| Talk {
                id: TalkId(deterministic_id(&[&title, name, stub_url.as_str()])),
                title: title.clone(),
                speaker: vec![speaker_hit(name)],
                date_time: None,
                abstract_text: None,
                topics: Vec::new(),
                media: Vec::new(),
                source: stub_source.clone(),
            })
            .collect();

        let media = detect_media(&html, &base_url);
        let access = classify_access(&html);

        let event_type = detect_event_type(&title);
        let id = event_id(&title, stub_url.as_str());
        let enriched = Event {
            id,
            title,
            url: Some(stub_url.clone()),
            event_type,
            status: EventStatus::Unknown,
            date: event_date,
            location,
            description,
            topics: Vec::new(),
            people,
            talks,
            media,
            access: AccessInfo {
                access,
                online: OnlineAvailability::Unknown,
            },
            sources: vec![stub_source],
            score: 0.0,
            score_components: ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        };
        Ok(EventCandidate {
            event: enriched,
            stub: event,
        })
    }
}

// --- helpers ---------------------------------------------------------------

fn require_selectors(source: &SourceSpec) -> Result<&HtmlSelectors, AdapterError> {
    source
        .selectors
        .as_ref()
        .ok_or_else(|| AdapterError::Parse {
            source_id: source.id.clone(),
            message: "configured HTML adapter requires selectors".into(),
        })
}

fn parse_selector(
    source_id: &str,
    field: &str,
    selector_str: &str,
) -> Result<Selector, AdapterError> {
    Selector::parse(selector_str).map_err(|e| AdapterError::Parse {
        source_id: source_id.to_string(),
        message: format!("invalid {field} selector {selector_str:?}: {e}"),
    })
}

/// Collect text from direct `Node::Text` children only, ignoring text inside
/// nested elements. Used for date extraction where a `<dt>` or `<span>` wraps
/// the date text alongside `<a>` links (e.g. ams-calendar's `dt.event_dates`
/// contains "Expand to view..." links after the date).
fn direct_text(element: &ElementRef) -> String {
    element
        .children()
        .filter_map(|child| match child.value() {
            Node::Text(t) => Some(&**t),
            _ => None,
        })
        .collect()
}

fn first_text(document: &Html, selector: &Selector) -> Option<String> {
    let element = document.select(selector).next()?;
    // Prefer direct text nodes so a titled element that wraps links/spans
    // (e.g. `<h1>Title <a>Live stream</a></h1>`) is not polluted by descendant
    // text. Fall back to all descendant text when the direct text is empty
    // (e.g. the title lives entirely inside a nested <span>).
    let direct = crate::helpers::clean_text(&direct_text(&element));
    if !direct.is_empty() {
        return Some(direct);
    }
    let text = crate::helpers::clean_text(&element.text().collect::<String>());
    if text.is_empty() { None } else { Some(text) }
}

fn first_date(document: &Html, selector: &Selector) -> Option<EventDate> {
    let element = document.select(selector).next()?;
    if let Some(dt) = element.attr("datetime")
        && let Ok(d) = parse_date(dt)
        && d.precision != DatePrecision::Unknown
    {
        return Some(d);
    }
    let direct = crate::helpers::clean_text(&direct_text(&element));
    if !direct.is_empty()
        && let Ok(d) = parse_date(&direct)
        && d.precision != DatePrecision::Unknown
    {
        return Some(d);
    }
    let text = crate::helpers::clean_text(&element.text().collect::<String>());
    if text.is_empty() {
        return None;
    }
    let d = parse_date(&text).ok()?;
    (d.precision != DatePrecision::Unknown).then_some(d)
}

fn first_text_in(scope: &ElementRef, selector: &Selector) -> Option<String> {
    let element = scope.select(selector).next()?;
    let text = crate::helpers::clean_text(&element.text().collect::<String>());
    if text.is_empty() { None } else { Some(text) }
}

fn first_date_in(scope: &ElementRef, selector: &Selector, year_hint: i32) -> Option<EventDate> {
    let element = scope.select(selector).next()?;
    if let Some(dt) = element.attr("datetime")
        && let Ok(d) = parse_date(dt)
        && d.precision != DatePrecision::Unknown
    {
        return Some(d);
    }
    let direct = crate::helpers::clean_text(&direct_text(&element));
    if !direct.is_empty()
        && let Ok(d) = parse_date_with_year_hint(&direct, year_hint)
        && d.precision != DatePrecision::Unknown
    {
        return Some(d);
    }
    let text = crate::helpers::clean_text(&element.text().collect::<String>());
    if text.is_empty() {
        return None;
    }
    let d = parse_date_with_year_hint(&text, year_hint).ok()?;
    (d.precision != DatePrecision::Unknown).then_some(d)
}

fn all_texts(document: &Html, selector: &Selector) -> Vec<String> {
    document
        .select(selector)
        .filter_map(|el| {
            let text = crate::helpers::clean_text(&el.text().collect::<String>());
            if text.is_empty() { None } else { Some(text) }
        })
        .collect()
}

fn optional_first_text(
    document: &Html,
    source_id: &str,
    field: &str,
    selector_str: Option<&str>,
) -> Result<Option<String>, AdapterError> {
    match selector_str {
        Some(s) => {
            let sel = parse_selector(source_id, field, s)?;
            Ok(first_text(document, &sel))
        }
        None => Ok(None),
    }
}

fn optional_all_texts(
    document: &Html,
    source_id: &str,
    field: &str,
    selector_str: Option<&str>,
) -> Result<Vec<String>, AdapterError> {
    match selector_str {
        Some(s) => {
            let sel = parse_selector(source_id, field, s)?;
            Ok(all_texts(document, &sel))
        }
        None => Ok(Vec::new()),
    }
}

fn parse_or_unknown(text: &str) -> EventDate {
    parse_date(text).unwrap_or_else(|_| EventDate::unknown(text.to_string()))
}

fn speaker_hit(name: &str) -> PersonHit {
    PersonHit {
        canonical_name: name.to_string(),
        matched_text: name.to_string(),
        role: PersonRole::Speaker,
        evidence: Some(name.to_string()),
        confidence: 1.0,
        scholar_tags: Vec::new(),
    }
}

fn build_minimal_event(stub: &EventStub) -> Event {
    Event {
        id: event_id(&stub.title, stub.url.as_str()),
        title: stub.title.clone(),
        url: Some(stub.url.clone()),
        event_type: detect_event_type(&stub.title),
        status: EventStatus::Unknown,
        date: stub
            .date_hint
            .clone()
            .unwrap_or_else(|| parse_or_unknown("")),
        location: None,
        description: None,
        topics: Vec::new(),
        people: Vec::new(),
        talks: Vec::new(),
        media: Vec::new(),
        access: AccessInfo {
            access: PublicAccess::Unknown,
            online: OnlineAvailability::Unknown,
        },
        sources: vec![stub.source.clone()],
        score: 0.0,
        score_components: ScoreComponents::default(),
        rank_reasons: Vec::new(),
        first_seen_at: None,
        last_seen_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radar_core::config::HtmlSelectors;
    use radar_core::{
        AdapterError, AdapterKind, DatePrecision, EventStub, EventType, FetchedDocument,
        SourceEvidence, SourceKind, SourceSpec, SourceTier,
    };
    use url::Url;

    // chrono is only a transitive dependency (via radar-core), so this crate
    // cannot name `DateTime<Utc>` directly. Obtain one by deserializing a
    // SourceEvidence whose `captured_at` is a serde DateTime — serde_json is a
    // direct dep and SourceEvidence: Deserialize.
    fn make_doc(url: &str, body: &str) -> FetchedDocument {
        let evidence: SourceEvidence = serde_json::from_str(
            r#"{"source_id":"x","source_url":"https://example.com/","captured_at":"2026-08-09T00:00:00Z"}"#,
        )
        .expect("SourceEvidence fixture parses");
        let fetched_at = evidence
            .captured_at
            .expect("captured_at present in fixture");
        FetchedDocument {
            url: Url::parse(url).expect("valid url"),
            final_url: Url::parse(url).expect("valid url"),
            status: 200,
            content_type: Some("text/html; charset=utf-8".into()),
            body: body.as_bytes().to_vec(),
            fetched_at,
        }
    }

    fn make_doc_bytes(url: &str, body: &[u8]) -> FetchedDocument {
        let evidence: SourceEvidence = serde_json::from_str(
            r#"{"source_id":"x","source_url":"https://example.com/","captured_at":"2026-08-09T00:00:00Z"}"#,
        )
        .expect("SourceEvidence fixture parses");
        let fetched_at = evidence
            .captured_at
            .expect("captured_at present in fixture");
        FetchedDocument {
            url: Url::parse(url).expect("valid url"),
            final_url: Url::parse(url).expect("valid url"),
            status: 200,
            content_type: Some("text/html; charset=iso-8859-1".into()),
            body: body.to_vec(),
            fetched_at,
        }
    }

    fn make_source(id: &str, selectors: Option<HtmlSelectors>) -> SourceSpec {
        SourceSpec {
            id: id.into(),
            name: id.into(),
            tier: SourceTier::default(),
            kind: SourceKind::default(),
            adapter: AdapterKind::HtmlConfig,
            entrypoint: None,
            allowed_hosts: Vec::new(),
            max_depth: 2,
            request_budget: 20,
            media_strategy: None,
            dynamic: false,
            enabled: true,
            fixture: None,
            selectors,
        }
    }

    fn test_selectors() -> HtmlSelectors {
        HtmlSelectors {
            list: ".event-list".into(),
            list_link: "a".into(),
            detail_title: "h1".into(),
            detail_date: "time".into(),
            detail_location: None,
            detail_description: None,
            detail_speaker: None,
            list_title: None,
            list_date: None,
        }
    }

    // --- discover ---

    #[test]
    fn discover_finds_links() {
        let html = r#"<!DOCTYPE html><html><body>
          <ul class="event-list">
            <li><a href="/talks/algebra">Algebra Seminar</a></li>
            <li><a href="/talks/geometry">Geometry Talk</a></li>
          </ul>
        </body></html>"#;
        let document = make_doc("https://example.com/events", html);
        let source = make_source("test", Some(test_selectors()));
        let stubs = HtmlConfigAdapter
            .discover(&document, &source)
            .expect("discover ok");
        assert_eq!(stubs.len(), 2);
        assert_eq!(stubs[0].title, "Algebra Seminar");
        assert_eq!(stubs[0].url.as_str(), "https://example.com/talks/algebra");
        assert_eq!(stubs[1].title, "Geometry Talk");
        assert_eq!(stubs[1].url.as_str(), "https://example.com/talks/geometry");
        assert!(stubs[0].date_hint.is_none());
        assert_eq!(stubs[0].source.source_id, "test");
    }

    #[test]
    fn discover_requires_selectors() {
        let document = make_doc("https://example.com/events", "<html></html>");
        let source = make_source("test", None);
        let result = HtmlConfigAdapter.discover(&document, &source);
        assert!(
            matches!(result, Err(AdapterError::Parse { .. })),
            "missing selectors must surface as Parse error, got {result:?}"
        );
    }

    #[test]
    fn discover_no_matches_returns_empty() {
        let html = r#"<html><body><div class="other">no events here</div></body></html>"#;
        let document = make_doc("https://example.com/events", html);
        let source = make_source("test", Some(test_selectors()));
        let stubs = HtmlConfigAdapter
            .discover(&document, &source)
            .expect("discover ok");
        assert!(stubs.is_empty());
    }

    #[test]
    fn discover_skips_links_without_href_or_title() {
        let html = r#"<html><body>
          <ul class="event-list">
            <li><a>no href</a></li>
            <li><a href="/ok">   </a></li>
            <li><a href="/real">Real Talk</a></li>
          </ul>
        </body></html>"#;
        let document = make_doc("https://example.com/events", html);
        let source = make_source("test", Some(test_selectors()));
        let stubs = HtmlConfigAdapter
            .discover(&document, &source)
            .expect("discover ok");
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].title, "Real Talk");
    }

    #[test]
    fn discover_rejects_invalid_list_selector() {
        let html =
            r#"<html><body><ul class="event-list"><li><a href="/x">T</a></li></ul></body></html>"#;
        let document = make_doc("https://example.com/events", html);
        let mut selectors = test_selectors();
        selectors.list = ".[bad".into();
        let source = make_source("test", Some(selectors));
        let result = HtmlConfigAdapter.discover(&document, &source);
        assert!(matches!(result, Err(AdapterError::Parse { .. })));
    }

    // --- plan_enrichment ---

    #[test]
    fn plan_enrichment_emits_single_detail_fetch() {
        let stub = EventStub {
            title: "T".into(),
            url: Url::parse("https://example.com/x").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test".into(),
                source_url: Url::parse("https://example.com/").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let source = make_source("test", None);
        let plans = HtmlConfigAdapter.plan_enrichment(&stub, &source);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].url.as_str(), "https://example.com/x");
        assert_eq!(plans[0].depth, 1);
        assert_eq!(plans[0].reason, "html_config_detail");
    }

    // --- enrich ---

    fn stub_for(title: &str, url: &str) -> EventStub {
        EventStub {
            title: title.into(),
            url: Url::parse(url).unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test".into(),
                source_url: Url::parse("https://example.com/events").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        }
    }

    #[test]
    fn enrich_requires_selectors() {
        let stub = stub_for("Standalone", "https://example.com/x");
        let source = make_source("test", None);
        let result = HtmlConfigAdapter.enrich(stub, &[], &source);
        assert!(matches!(result, Err(AdapterError::Parse { .. })));
    }

    #[test]
    fn enrich_empty_documents_builds_minimal_event() {
        let stub = stub_for("Standalone", "https://example.com/x");
        let source = make_source("test", Some(test_selectors()));
        let candidate = HtmlConfigAdapter
            .enrich(stub, &[], &source)
            .expect("enrich ok");
        assert_eq!(candidate.event.title, "Standalone");
        assert!(candidate.event.location.is_none());
        assert!(candidate.event.description.is_none());
        assert!(candidate.event.people.is_empty());
        assert!(candidate.event.talks.is_empty());
        assert!(candidate.event.media.is_empty());
        assert_eq!(candidate.event.sources.len(), 1);
        assert_eq!(candidate.event.sources[0].source_id, "test");
        assert_eq!(candidate.stub.title, "Standalone");
    }

    #[test]
    fn enrich_extracts_fields_from_detail() {
        let html = r#"<!DOCTYPE html><html><body>
          <h1 class="title">Algebra Seminar</h1>
          <time datetime="2026-08-09">August 9, 2026</time>
          <span class="location">Room 101</span>
          <div class="description">A talk on algebra.</div>
          <div class="speaker">Alice Smith</div>
        </body></html>"#;
        let detail = make_doc("https://example.com/talks/algebra", html);
        let stub = stub_for("Algebra Seminar", "https://example.com/talks/algebra");
        let mut selectors = test_selectors();
        selectors.detail_title = "h1.title".into();
        selectors.detail_location = Some(".location".into());
        selectors.detail_description = Some(".description".into());
        selectors.detail_speaker = Some(".speaker".into());
        let source = make_source("test", Some(selectors));
        let candidate = HtmlConfigAdapter
            .enrich(stub, std::slice::from_ref(&detail), &source)
            .expect("enrich ok");

        assert_eq!(candidate.event.title, "Algebra Seminar");
        assert_eq!(candidate.event.event_type, EventType::Seminar);
        assert_eq!(candidate.event.date.precision, DatePrecision::Day);
        let location = candidate
            .event
            .location
            .as_ref()
            .expect("location extracted");
        assert_eq!(location.name, "Room 101");
        assert_eq!(
            candidate.event.description.as_deref(),
            Some("A talk on algebra.")
        );
        assert_eq!(candidate.event.people.len(), 1);
        assert_eq!(candidate.event.people[0].canonical_name, "Alice Smith");
        assert_eq!(candidate.event.people[0].role, PersonRole::Speaker);
        assert_eq!(candidate.event.talks.len(), 1);
        assert_eq!(candidate.event.talks[0].speaker.len(), 1);
        assert_eq!(
            candidate.event.talks[0].speaker[0].role,
            PersonRole::Speaker
        );
        assert_eq!(candidate.stub.title, "Algebra Seminar");
    }

    #[test]
    fn enrich_optional_selector_no_match_is_none() {
        // detail_location selector present but matches nothing -> location None.
        let html = r#"<html><body><h1>T</h1><time>2026-08-09</time></body></html>"#;
        let detail = make_doc("https://example.com/x", html);
        let stub = stub_for("T", "https://example.com/x");
        let mut selectors = test_selectors();
        selectors.detail_location = Some(".missing".into());
        let source = make_source("test", Some(selectors));
        let candidate = HtmlConfigAdapter
            .enrich(stub, std::slice::from_ref(&detail), &source)
            .expect("enrich ok");
        assert!(candidate.event.location.is_none());
        assert!(candidate.event.people.is_empty());
    }

    #[test]
    fn enrich_invalid_optional_selector_is_error() {
        let html = r#"<html><body><h1>T</h1><time>2026-08-09</time></body></html>"#;
        let detail = make_doc("https://example.com/x", html);
        let stub = stub_for("T", "https://example.com/x");
        let mut selectors = test_selectors();
        selectors.detail_location = Some(".[bad".into());
        let source = make_source("test", Some(selectors));
        let result = HtmlConfigAdapter.enrich(stub, std::slice::from_ref(&detail), &source);
        assert!(matches!(result, Err(AdapterError::Parse { .. })));
    }

    #[test]
    fn first_date_prefers_datetime_attr_over_text() {
        let html = r#"<html><body>
          <time datetime="2026-09-15">Some unrelated display text</time>
        </body></html>"#;
        let document = make_doc("https://example.com/x", html);
        let body = std::str::from_utf8(&document.body).expect("utf8");
        let parsed = Html::parse_document(body);
        let selector = Selector::parse("time").expect("valid selector");
        let date = first_date(&parsed, &selector).expect("date present");
        assert_eq!(date.precision, DatePrecision::Day);
        assert_eq!(
            date.start_date().map(|d| d.to_string()),
            Some("2026-09-15".into())
        );
    }

    #[test]
    fn first_date_falls_back_to_text_when_no_datetime_attr() {
        let html = r#"<html><body><time>2026-09-15</time></body></html>"#;
        let document = make_doc("https://example.com/x", html);
        let body = std::str::from_utf8(&document.body).expect("utf8");
        let parsed = Html::parse_document(body);
        let selector = Selector::parse("time").expect("valid selector");
        let date = first_date(&parsed, &selector).expect("date present");
        assert_eq!(date.precision, DatePrecision::Day);
        assert_eq!(
            date.start_date().map(|d| d.to_string()),
            Some("2026-09-15".into())
        );
    }

    #[test]
    fn first_date_rejects_unknown_precision() {
        let html = r#"<html><body><time>not a date at all</time></body></html>"#;
        let document = make_doc("https://example.com/x", html);
        let body = std::str::from_utf8(&document.body).expect("utf8");
        let parsed = Html::parse_document(body);
        let selector = Selector::parse("time").expect("valid selector");
        assert!(first_date(&parsed, &selector).is_none());
    }

    #[test]
    fn enrich_preserves_date_hint_when_detail_date_missing() {
        let html = r#"<html><body>
          <h1>Princeton Talk</h1>
          <time>no parseable date here</time>
        </body></html>"#;
        let detail = make_doc("https://example.com/talks/x", html);
        let mut stub = stub_for("Princeton Talk", "https://example.com/talks/x");
        stub.date_hint = parse_date("2026-09-20").ok();
        let source = make_source("test", Some(test_selectors()));
        let candidate = HtmlConfigAdapter
            .enrich(stub, std::slice::from_ref(&detail), &source)
            .expect("enrich ok");
        assert_eq!(candidate.event.date.precision, DatePrecision::Day);
        assert_eq!(
            candidate.event.date.start_date().map(|d| d.to_string()),
            Some("2026-09-20".into())
        );
    }

    #[test]
    fn enrich_detail_date_overrides_date_hint() {
        let html = r#"<html><body>
          <h1>Talk</h1>
          <time datetime="2026-10-01">Oct 1</time>
        </body></html>"#;
        let detail = make_doc("https://example.com/x", html);
        let mut stub = stub_for("Talk", "https://example.com/x");
        stub.date_hint = parse_date("2026-09-20").ok();
        let source = make_source("test", Some(test_selectors()));
        let candidate = HtmlConfigAdapter
            .enrich(stub, std::slice::from_ref(&detail), &source)
            .expect("enrich ok");
        assert_eq!(
            candidate.event.date.start_date().map(|d| d.to_string()),
            Some("2026-10-01".into())
        );
    }

    #[test]
    fn doc_body_valid_utf8_is_borrowed() {
        let body = b"<html><body>hello</body></html>";
        let s = crate::helpers::doc_body(body);
        assert!(matches!(s, std::borrow::Cow::Borrowed(_)));
        assert_eq!(s.as_ref(), "<html><body>hello</body></html>");
    }

    #[test]
    fn doc_body_invalid_utf8_replaces_with_replacement_char() {
        // 0xf6 is 'ö' in ISO-8859-1, invalid in UTF-8.
        let body: &[u8] = b"<html><body>G\xf6del Seminar</body></html>";
        let s = crate::helpers::doc_body(body);
        assert!(matches!(s, std::borrow::Cow::Owned(_)));
        assert_eq!(s.as_ref(), "<html><body>G\u{FFFD}del Seminar</body></html>");
    }

    #[test]
    fn discover_handles_non_utf8_body() {
        // Simulate an ISO-8859-1 page with a non-UTF-8 byte in the link text.
        let body: &[u8] =
            b"<html><body><ul class=\"event-list\"><li><a href=\"/talks/godel\">G\xf6del Talk</a></li></ul></body></html>";
        let document = make_doc_bytes("https://example.com/events", body);
        let source = make_source("test", Some(test_selectors()));
        let stubs = HtmlConfigAdapter
            .discover(&document, &source)
            .expect("discover ok with non-utf8 body");
        assert_eq!(stubs.len(), 1);
        assert!(stubs[0].title.contains("Gdel") || stubs[0].title.contains("G\u{FFFD}del"));
        assert_eq!(stubs[0].url.as_str(), "https://example.com/talks/godel");
    }
}
