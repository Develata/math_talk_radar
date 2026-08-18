//! RSS/Atom feed adapter (§P-5 structured-source priority #2).
//!
//! Parses RSS 2.0, RSS 1.0, Atom, and JSON Feed via `feed-rs`, which is
//! XXE-safe (built on quick-xml with no external-entity expansion). `discover`
//! maps feed entries to [`EventStub`]s; `enrich` fetches the entry's detail
//! page (when the coordinator supplies one) and fills the [`Event`] from the
//! shared HTML helpers. Speakers come from feed-level `entry.authors`
//! (`<dc:creator>` / `<author>`) and are intentionally not promoted from title
//! text (§P-2, §6.2).
use url::Url;

use radar_core::date::parse_date;
use radar_core::{
    AccessInfo, AdapterError, Event, EventCandidate, EventDate, EventStatus, EventStub, FetchPlan,
    FetchedDocument, Location, MediaId, MediaResource, MediaType, OnlineAvailability, PublicAccess,
    ScoreComponents, SourceAdapter, SourceEvidence, SourceSpec, deterministic_id, event_id,
};

use crate::helpers;

#[derive(Debug, Default)]
pub struct RssAdapter;

impl SourceAdapter for RssAdapter {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        let feed =
            feed_rs::parser::parse(document.body.as_slice()).map_err(|e| AdapterError::Parse {
                source_id: source.id.clone(),
                message: format!("{e}"),
            })?;

        let stubs = feed
            .entries
            .into_iter()
            .filter_map(|entry| {
                let title = entry.title?.content;
                let link = entry
                    .links
                    .iter()
                    .find(|l| l.rel.as_deref().map_or(true, |r| r == "alternate"))
                    .or_else(|| entry.links.first())?;
                let url = Url::parse(&link.href).ok()?;
                let date_hint = entry.published.or(entry.updated).map(|dt| {
                    parse_date(&dt.date_naive().to_string())
                        .unwrap_or_else(|_| EventDate::unknown(String::new()))
                });
                let native_id = (!entry.id.is_empty()).then_some(entry.id);
                Some(EventStub {
                    title,
                    url,
                    date_hint,
                    source: SourceEvidence {
                        source_id: source.id.clone(),
                        source_url: document.final_url.clone(),
                        evidence: None,
                        captured_at: Some(document.fetched_at),
                        native_id,
                    },
                })
            })
            .collect();
        Ok(stubs)
    }

    fn plan_enrichment(&self, event: &EventStub, source: &SourceSpec) -> Vec<FetchPlan> {
        if source.media_strategy.as_deref() == Some("youtube_channel") {
            return Vec::new();
        }
        vec![FetchPlan {
            url: event.url.clone(),
            depth: 1,
            reason: "rss_detail".into(),
        }]
    }

    fn enrich(
        &self,
        stub: EventStub,
        documents: &[FetchedDocument],
        source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        if source.media_strategy.as_deref() == Some("youtube_channel") {
            return Self::enrich_youtube(stub);
        }

        let (fields, media, access) = match documents.first() {
            Some(doc)
                if doc
                    .content_type
                    .as_deref()
                    .is_some_and(|ct| ct.contains("html")) =>
            {
                let body = crate::helpers::doc_body(&doc.body);
                let document = scraper::Html::parse_document(&body);
                (
                    helpers::extract_html_fields(&document),
                    helpers::detect_media(&document, &doc.final_url, &source.id),
                    helpers::classify_access(&document),
                )
            }
            _ => (
                helpers::HtmlFields::default(),
                Vec::new(),
                PublicAccess::Unknown,
            ),
        };

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
            location: fields.location_text.map(|name| Location {
                name,
                city: None,
                country: None,
                venue: None,
            }),
            description: fields.description,
            topics: Vec::new(),
            people: Vec::new(),
            talks: Vec::new(),
            media,
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

impl RssAdapter {
    /// §20 Media Plane: build an Event from a YouTube RSS stub without fetching
    /// a detail page. Each video becomes a recorded-talk Event carrying one
    /// `MediaResource { media_type: Video, platform: "youtube" }`. The RSS
    /// entry already provides title, watch URL, and publication date; no
    /// enrichment fetch is needed (`plan_enrichment` returns empty for
    /// `youtube_channel`).
    fn enrich_youtube(stub: EventStub) -> Result<EventCandidate, AdapterError> {
        let date = stub
            .date_hint
            .clone()
            .unwrap_or_else(|| EventDate::unknown(String::new()));

        let media = MediaResource {
            id: MediaId(deterministic_id(&[stub.url.as_str()])),
            media_type: MediaType::Video,
            title: Some(stub.title.clone()),
            url: stub.url.clone(),
            platform: Some("youtube".to_string()),
            public_access: PublicAccess::Open,
            published_at: None,
            source: stub.source.clone(),
        };

        let event = Event {
            id: event_id(&stub.title, stub.url.as_str()),
            title: stub.title.clone(),
            url: Some(stub.url.clone()),
            event_type: helpers::detect_event_type(&stub.title),
            status: EventStatus::Unknown,
            date,
            location: None,
            description: None,
            topics: Vec::new(),
            people: Vec::new(),
            talks: Vec::new(),
            media: vec![media],
            access: AccessInfo {
                access: PublicAccess::Open,
                online: OnlineAvailability::RecordingAvailable,
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

#[cfg(test)]
mod tests {
    use super::*;
    use radar_core::{AdapterKind, EventType, MediaType, SourceKind, SourceTier};

    fn test_source() -> SourceSpec {
        SourceSpec {
            id: "test-rss".to_string(),
            name: "Test RSS".to_string(),
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

    fn make_doc(body: &str, content_type: &str) -> FetchedDocument {
        FetchedDocument {
            url: Url::parse("https://example.com/feed.xml").unwrap(),
            final_url: Url::parse("https://example.com/feed.xml").unwrap(),
            status: 200,
            content_type: Some(content_type.to_string()),
            body: body.as_bytes().to_vec(),
            // chrono is not a direct dependency of radar-adapters; rely on the
            // `From<SystemTime> for DateTime<Utc>` impl exposed transitively via
            // feed-rs, with the target type inferred from the field.
            fetched_at: std::time::SystemTime::now().into(),
        }
    }

    const SAMPLE_RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Math Talks</title>
    <link>https://example.com/</link>
    <description>Mathematics talks feed</description>
    <item>
      <title>Conference on Algebra</title>
      <link>https://example.com/talks/1</link>
      <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
    </item>
    <item>
      <title>Workshop on Graph Theory</title>
      <link>https://example.com/talks/2</link>
      <pubDate>Tue, 02 Jan 2024 00:00:00 +0000</pubDate>
    </item>
    <item>
      <title>Seminar on Number Theory</title>
      <link>https://example.com/talks/3</link>
      <pubDate>Wed, 03 Jan 2024 00:00:00 +0000</pubDate>
    </item>
  </channel>
</rss>"#;

    #[test]
    fn discover_minimal_rss() {
        let doc = make_doc(SAMPLE_RSS, "application/rss+xml");
        let source = test_source();
        let stubs = RssAdapter
            .discover(&doc, &source)
            .expect("valid RSS feed should parse");
        assert!(
            stubs.len() >= 3,
            "expected at least 3 stubs, got {}",
            stubs.len()
        );
        assert_eq!(stubs[0].title, "Conference on Algebra");
        assert_eq!(stubs[0].url.as_str(), "https://example.com/talks/1");
        assert_eq!(stubs[1].title, "Workshop on Graph Theory");
        assert_eq!(stubs[2].title, "Seminar on Number Theory");
        assert_eq!(stubs[0].source.source_id, "test-rss");
        assert_eq!(
            stubs[0].source.source_url.as_str(),
            "https://example.com/feed.xml"
        );
        assert!(
            stubs[0].date_hint.is_some(),
            "pubDate should yield a date hint"
        );
    }

    #[test]
    fn discover_malformed_feed_errors() {
        let doc = make_doc("<<<not a feed>>>", "application/rss+xml");
        let source = test_source();
        let result = RssAdapter.discover(&doc, &source);
        assert!(
            matches!(result, Err(AdapterError::Parse { .. })),
            "malformed feed should return AdapterError::Parse, got {result:?}"
        );
    }

    #[test]
    fn discover_empty_body_errors() {
        let doc = make_doc("", "application/rss+xml");
        let source = test_source();
        let result = RssAdapter.discover(&doc, &source);
        assert!(
            matches!(result, Err(AdapterError::Parse { .. })),
            "empty body should return AdapterError::Parse, got {result:?}"
        );
    }

    #[test]
    fn plan_enrichment_requests_detail() {
        let stub = EventStub {
            title: "Conference on Algebra".to_string(),
            url: Url::parse("https://example.com/talks/1").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-rss".to_string(),
                source_url: Url::parse("https://example.com/feed.xml").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let source = test_source();
        let plans = RssAdapter.plan_enrichment(&stub, &source);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].url.as_str(), "https://example.com/talks/1");
        assert_eq!(plans[0].depth, 1);
        assert_eq!(plans[0].reason, "rss_detail");
    }

    #[test]
    fn enrich_without_documents_is_minimal() {
        let stub = EventStub {
            title: "Workshop on Graph Theory".to_string(),
            url: Url::parse("https://example.com/talks/2").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-rss".to_string(),
                source_url: Url::parse("https://example.com/feed.xml").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let source = test_source();
        let candidate = RssAdapter
            .enrich(stub, &[], &source)
            .expect("enrich with no documents should still produce a minimal event");
        assert_eq!(candidate.event.title, "Workshop on Graph Theory");
        assert_eq!(candidate.event.event_type, EventType::Workshop);
        assert_eq!(candidate.event.status, EventStatus::Unknown);
        assert!(candidate.event.description.is_none());
        assert!(candidate.event.location.is_none());
        assert!(candidate.event.media.is_empty());
        assert_eq!(candidate.event.access.access, PublicAccess::Unknown);
        assert_eq!(candidate.event.sources.len(), 1);
        assert_eq!(candidate.stub.title, "Workshop on Graph Theory");
    }

    #[test]
    fn enrich_with_html_detail_extracts_fields() {
        let stub = EventStub {
            title: "Seminar on Number Theory".to_string(),
            url: Url::parse("https://example.com/talks/3").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-rss".to_string(),
                source_url: Url::parse("https://example.com/feed.xml").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let html = r#"<!DOCTYPE html>
<html><head>
<meta name="description" content="A seminar on number theory">
</head><body>
<h1>Seminar on Number Theory</h1>
<time datetime="2024-03-15">March 15, 2024</time>
<div class="location">MIT, Cambridge, MA</div>
<a href="https://example.com/slides.pdf">Slides</a>
</body></html>"#;
        let doc = make_doc(html, "text/html; charset=utf-8");
        let source = test_source();
        let candidate = RssAdapter
            .enrich(stub, std::slice::from_ref(&doc), &source)
            .expect("enrich with an HTML detail document should succeed");
        assert_eq!(candidate.event.event_type, EventType::Seminar);
        assert_eq!(
            candidate.event.description.as_deref(),
            Some("A seminar on number theory")
        );
        assert_eq!(
            candidate.event.location.as_ref().unwrap().name,
            "MIT, Cambridge, MA"
        );
        assert_eq!(candidate.event.media.len(), 1);
        assert_eq!(candidate.event.media[0].media_type, MediaType::Slides);
    }
}
