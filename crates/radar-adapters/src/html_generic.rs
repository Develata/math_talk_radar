//! Generic HTML fallback adapter (§P-5 priority #6, last resort).
//!
//! Per §74, the generic `<a>` parser must never be the primary strategy; it is
//! only used when no structured or configured adapter applies (Metis D5: no
//! automatic fallback from site-specific adapters). `discover` finds event-like
//! links by keyword + date-like signal matching with a navigation filter
//! (§47: precision ≥90%). `enrich` extracts fields from the detail page using
//! the shared HTML helpers.

use scraper::Html;

use radar_core::date::parse_date;
use radar_core::{
    AccessInfo, AdapterError, Event, EventCandidate, EventDate, EventStatus, EventStub, FetchPlan,
    FetchedDocument, Location, OnlineAvailability, PublicAccess, ScoreComponents, SourceAdapter,
    SourceEvidence, SourceSpec, event_id,
};

use crate::helpers;

#[derive(Debug, Default)]
pub struct HtmlGenericAdapter;

// ===========================================================================
// Navigation filter (§47)
// ===========================================================================

/// Lowercase exact-match stopword list for obvious navigation links. A link
/// whose trimmed text exactly equals one of these is never promoted to an
/// [`EventStub`].
const NAV_STOPWORDS: &[&str] = &[
    "home",
    "about",
    "contact",
    "login",
    "sign in",
    "register",
    "sign up",
    "menu",
    "navigation",
    "search",
    "help",
    "faq",
    "privacy",
    "terms",
    "sitemap",
    "back",
    "next",
    "previous",
    "more",
];

/// True if `href` is a navigation/dead-end target: fragment, script, mail/tel,
/// bare root, or empty.
fn is_nav_href(href: &str) -> bool {
    let trimmed = href.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with('#')
        || lower.starts_with("javascript:")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
}

/// True if `text` (trimmed, lowercased) exactly equals a navigation stopword.
fn is_nav_text(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    NAV_STOPWORDS.contains(&lower.as_str())
}

// ===========================================================================
// Event keyword + date-like signal
// ===========================================================================

/// Event-type keywords covering all 14 [`radar_core::EventType`] variants
/// (§5.2). Case-insensitive substring match against link text.
const EVENT_KEYWORDS: &[&str] = &[
    "conference",
    "workshop",
    "seminar",
    "lecture",
    "summer school",
    "winter school",
    "spring school",
    "colloquium",
    "panel",
    "mini course",
    "minicourse",
    "short course",
    "research program",
    "research programme",
    "thematic program",
    "award lecture",
    "prize lecture",
    "memorial",
    "symposium",
    "meeting",
    "school",
];

/// Full English month names for the secondary date-like signal.
const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// True if `text` contains a 4-digit year (19xx or 20xx).
fn contains_year(text: &str) -> bool {
    text.as_bytes().windows(4).any(|w| {
        ((w[0] == b'1' && w[1] == b'9') || (w[0] == b'2' && w[1] == b'0'))
            && w[2].is_ascii_digit()
            && w[3].is_ascii_digit()
    })
}

/// True if `text` contains a date-like substring: a 4-digit year or a full
/// English month name. Secondary signal — the navigation filter runs first.
fn contains_date_like(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if MONTHS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    contains_year(text)
}

/// True if `text` contains an event-type keyword (case-insensitive substring).
fn contains_event_keyword(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    EVENT_KEYWORDS.iter().any(|k| lower.contains(k))
}

// ===========================================================================
// Text + event helpers
// ===========================================================================

/// Build a minimal [`Event`] from a stub when no detail document is available.
fn minimal_event_from_stub(stub: &EventStub) -> Event {
    let date = stub
        .date_hint
        .clone()
        .unwrap_or_else(|| EventDate::unknown(String::new()));
    Event {
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

// ===========================================================================
// SourceAdapter impl
// ===========================================================================

impl SourceAdapter for HtmlGenericAdapter {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        let body = String::from_utf8_lossy(&document.body);
        if body.is_empty() {
            return Ok(Vec::new());
        }
        let parsed = Html::parse_document(&body);
        let Some(selector) = helpers::cached_selector("a") else {
            return Ok(Vec::new());
        };
        let base_url = &document.url;
        let mut stubs = Vec::new();
        for element in parsed.select(selector) {
            let Some(href) = element.attr("href") else {
                continue;
            };
            let text = crate::helpers::clean_text(&element.text().collect::<String>());
            if text.is_empty() {
                continue;
            }
            // Nav filter (§47): reject obvious navigation/dead-end links.
            if is_nav_text(&text) || is_nav_href(href) {
                continue;
            }
            // Event signal: keyword match OR date-like secondary signal.
            if !contains_event_keyword(&text) && !contains_date_like(&text) {
                continue;
            }
            // Resolve relative URL.
            let Ok(resolved) = base_url.join(href) else {
                continue;
            };
            let evidence = text.clone();
            stubs.push(EventStub {
                title: text,
                url: resolved,
                date_hint: None,
                source: SourceEvidence {
                    source_id: source.id.clone(),
                    source_url: base_url.clone(),
                    evidence: Some(evidence),
                    captured_at: None,
                    native_id: None,
                },
            });
        }
        Ok(stubs)
    }

    fn plan_enrichment(&self, event: &EventStub, _source: &SourceSpec) -> Vec<FetchPlan> {
        vec![FetchPlan {
            url: event.url.clone(),
            depth: 1,
            reason: "html_generic_detail".into(),
        }]
    }

    fn enrich(
        &self,
        stub: EventStub,
        documents: &[FetchedDocument],
        _source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        let event = match documents.first() {
            None => minimal_event_from_stub(&stub),
            Some(doc) => {
                let body = crate::helpers::doc_body(&doc.body);
                let base_url = &doc.url;
                let document = scraper::Html::parse_document(&body);
                let fields = helpers::extract_html_fields(&document, base_url);
                let media = helpers::detect_media(&document, base_url);
                let access = helpers::classify_access(&document);

                // Event type from stub title + extracted description text.
                let title = fields.title.unwrap_or_else(|| stub.title.clone());
                let event_type = match fields.description.as_deref() {
                    Some(d) => helpers::detect_event_type(&format!("{title} {d}")),
                    None => helpers::detect_event_type(&title),
                };

                // Date: prefer detail page, fall back to stub hint, else Unknown.
                let date = match fields.date_text.as_deref() {
                    Some(t) => parse_date(t).unwrap_or_else(|_| EventDate::unknown(String::new())),
                    None => stub
                        .date_hint
                        .clone()
                        .unwrap_or_else(|| EventDate::unknown(String::new())),
                };

                let location = fields.location_text.map(|name| Location {
                    name,
                    city: None,
                    country: None,
                    venue: None,
                });

                Event {
                    id: event_id(&stub.title, stub.url.as_str()),
                    title,
                    url: Some(stub.url.clone()),
                    event_type,
                    status: EventStatus::Unknown,
                    date,
                    location,
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
                }
            }
        };

        Ok(EventCandidate { event, stub })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use radar_core::{AdapterKind, EventType, SourceKind, SourceTier};
    use url::Url;

    fn make_doc(url: &str, body: &str) -> FetchedDocument {
        FetchedDocument {
            url: Url::parse(url).expect("valid url"),
            final_url: Url::parse(url).expect("valid url"),
            status: 200,
            content_type: Some("text/html; charset=utf-8".into()),
            body: body.as_bytes().to_vec(),
            // chrono is not a direct dependency of radar-adapters; rely on the
            // `From<SystemTime> for DateTime<Utc>` impl with the target type
            // inferred from the field.
            fetched_at: std::time::SystemTime::now().into(),
        }
    }

    fn test_source() -> SourceSpec {
        SourceSpec {
            id: "test-html-generic".to_string(),
            name: "Test HTML Generic".to_string(),
            tier: SourceTier::default(),
            kind: SourceKind::default(),
            adapter: AdapterKind::HtmlGeneric,
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

    // 10 event-like links + 5 nav links -> >=5 discovered, no nav links.
    #[test]
    fn discover_finds_events_and_filters_nav() {
        let html = r#"<html><body>
          <nav>
            <a href="/">Home</a>
            <a href="/about">About</a>
            <a href="/contact">Contact</a>
            <a href="/login">Login</a>
            <a href="/search">Search</a>
          </nav>
          <ul>
            <li><a href="/e1">Conference on Algebra</a></li>
            <li><a href="/e2">Workshop on Graph Theory</a></li>
            <li><a href="/e3">Seminar on Number Theory</a></li>
            <li><a href="/e4">Colloquium Talk</a></li>
            <li><a href="/e5">Summer School on Topology</a></li>
            <li><a href="/e6">Mini Course on Category Theory</a></li>
            <li><a href="/e7">Panel Discussion</a></li>
            <li><a href="/e8">Award Lecture Series</a></li>
            <li><a href="/e9">Memorial Conference</a></li>
            <li><a href="/e10">Research Program on Geometry</a></li>
          </ul>
        </body></html>"#;
        let doc = make_doc("https://example.com/events", html);
        let source = test_source();
        let discovered = HtmlGenericAdapter
            .discover(&doc, &source)
            .expect("discover should succeed");
        assert!(
            discovered.len() >= 5,
            "expected at least 5 event stubs, got {}",
            discovered.len()
        );
        // Precision: no nav links promoted.
        const NAV_TEXTS: &[&str] = &["home", "about", "contact", "login", "search"];
        for stub in &discovered {
            let lower = stub.title.trim().to_ascii_lowercase();
            assert!(
                !NAV_TEXTS.contains(&lower.as_str()),
                "nav link '{}' should not be promoted to EventStub",
                stub.title
            );
        }
    }

    // Page with only nav links -> empty result.
    #[test]
    fn discover_only_nav_links_returns_empty() {
        let html = r##"<html><body>
          <a href="/">Home</a>
          <a href="/about">About</a>
          <a href="#top">Back</a>
          <a href="javascript:void(0)">Menu</a>
          <a href="/search">Search</a>
        </body></html>"##;
        let doc = make_doc("https://example.com/events", html);
        let source = test_source();
        let discovered = HtmlGenericAdapter
            .discover(&doc, &source)
            .expect("discover should succeed");
        assert!(
            discovered.is_empty(),
            "nav-only page should yield no stubs, got {discovered:?}"
        );
    }

    // Empty HTML -> empty result.
    #[test]
    fn discover_empty_html_returns_empty() {
        let doc = make_doc("https://example.com/events", "");
        let source = test_source();
        let discovered = HtmlGenericAdapter
            .discover(&doc, &source)
            .expect("discover should succeed");
        assert!(discovered.is_empty());
    }

    // plan_enrichment emits a single depth-1 detail fetch.
    #[test]
    fn plan_enrichment_emits_detail_fetch() {
        let stub = EventStub {
            title: "Conference on Algebra".into(),
            url: Url::parse("https://example.com/e1").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-html-generic".into(),
                source_url: Url::parse("https://example.com/events").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let source = test_source();
        let plans = HtmlGenericAdapter.plan_enrichment(&stub, &source);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].url.as_str(), "https://example.com/e1");
        assert_eq!(plans[0].depth, 1);
        assert_eq!(plans[0].reason, "html_generic_detail");
    }

    // enrich with no documents builds a minimal event from the stub.
    #[test]
    fn enrich_empty_documents_builds_minimal_event() {
        let stub = EventStub {
            title: "Workshop on Graph Theory".into(),
            url: Url::parse("https://example.com/e2").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-html-generic".into(),
                source_url: Url::parse("https://example.com/events").unwrap(),
                evidence: None,
                captured_at: None,
                native_id: None,
            },
        };
        let source = test_source();
        let candidate = HtmlGenericAdapter
            .enrich(stub, &[], &source)
            .expect("enrich with no documents should still produce a minimal event");
        assert_eq!(candidate.event.title, "Workshop on Graph Theory");
        assert_eq!(candidate.event.event_type, EventType::Workshop);
        assert!(candidate.event.description.is_none());
        assert!(candidate.event.location.is_none());
        assert!(candidate.event.media.is_empty());
        assert_eq!(candidate.event.access.access, PublicAccess::Unknown);
        assert_eq!(candidate.stub.title, "Workshop on Graph Theory");
    }
}
