//! Integration tests for radar-adapters covering the five source-adapter
//! kinds (SRC-001..005), media/access helpers (MED-001..003), JSON-LD speaker
//! extraction (TALK-001), and §67 input-safety guards (XXE, ICS depth,
//! malformed ICS).
//!
//! Fixtures are sanitized, offline, and contain no real-website data (§45).

use radar_adapters::helpers;
use radar_adapters::html_config::HtmlConfigAdapter;
use radar_adapters::html_generic::HtmlGenericAdapter;
use radar_adapters::ics::IcsAdapter;
use radar_adapters::jsonld::JsonLdAdapter;
use radar_adapters::rss::RssAdapter;
use radar_core::config::HtmlSelectors;
use radar_core::{
    AdapterError, AdapterKind, FetchedDocument, MediaType, PersonRole, PublicAccess, SourceAdapter,
    SourceKind, SourceSpec, SourceTier,
};
use scraper::Html;
use url::Url;

const RSS_FEED: &str = include_str!("fixtures/rss_feed.xml");
const ICS_CALENDAR: &str = include_str!("fixtures/ics_calendar.ics");
const JSONLD_PAGE: &str = include_str!("fixtures/jsonld_page.html");
const CONFIG_HTML_PAGE: &str = include_str!("fixtures/config_html_page.html");
const GENERIC_HTML_PAGE: &str = include_str!("fixtures/generic_html_page.html");
const XXE_ATTACK: &str = include_str!("fixtures/xxe_attack.xml");
const DEEP_NESTED_ICS: &str = include_str!("fixtures/deep_nested_ics.ics");
const MEDIA_DETECTION: &str = include_str!("fixtures/media_detection.html");
const MALFORMED_ICS: &str = include_str!("fixtures/malformed_ics.ics");

/// Build a `FetchedDocument` from a fixture body and content type.
/// `fetched_at` uses `SystemTime::now().into()` because chrono is only a
/// transitive dependency (via radar-core); the `From<SystemTime> for
/// `DateTime<Utc>` impl is available with the target type inferred from the
/// field, matching the pattern used by the inline adapter tests.
fn make_doc(body: &str, content_type: &str) -> FetchedDocument {
    FetchedDocument {
        url: Url::parse("https://example.com/test").unwrap(),
        final_url: Url::parse("https://example.com/test").unwrap(),
        status: 200,
        content_type: Some(content_type.into()),
        body: body.as_bytes().to_vec(),
        fetched_at: std::time::SystemTime::now().into(),
    }
}

/// Build a `SourceSpec` with the given adapter/kind and no selectors.
fn make_source(id: &str, adapter: AdapterKind, kind: SourceKind) -> SourceSpec {
    SourceSpec {
        id: id.to_string(),
        name: id.to_string(),
        tier: SourceTier::Unknown,
        kind,
        adapter,
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

// ===========================================================================
// SRC-001: RSS adapter
// ===========================================================================

#[test]
fn src_001_rss_discovers_at_least_ten() {
    let doc = make_doc(RSS_FEED, "application/rss+xml");
    let source = make_source("src-001", AdapterKind::Rss, SourceKind::RssFeed);
    let discovered = RssAdapter
        .discover(&doc, &source)
        .expect("RSS feed should parse");
    assert!(
        discovered.len() >= 10,
        "expected >=10 stubs, got {}",
        discovered.len()
    );
}

// ===========================================================================
// SRC-002: ICS adapter
// ===========================================================================

#[test]
fn src_002_ics_discovers_at_least_ten() {
    let doc = make_doc(ICS_CALENDAR, "text/calendar");
    let source = make_source("src-002", AdapterKind::Ics, SourceKind::IcsFeed);
    let discovered = IcsAdapter
        .discover(&doc, &source)
        .expect("ICS calendar should parse");
    assert!(
        discovered.len() >= 10,
        "expected >=10 stubs, got {}",
        discovered.len()
    );
}

// ===========================================================================
// SRC-003: JSON-LD adapter
// ===========================================================================

#[test]
fn src_003_jsonld_discovers_at_least_ten() {
    let doc = make_doc(JSONLD_PAGE, "text/html");
    let source = make_source("src-003", AdapterKind::JsonLd, SourceKind::JsonLd);
    let discovered = JsonLdAdapter
        .discover(&doc, &source)
        .expect("JSON-LD page should parse");
    assert!(
        discovered.len() >= 10,
        "expected >=10 stubs, got {}",
        discovered.len()
    );
}

// ===========================================================================
// SRC-004: Configured HTML adapter
// ===========================================================================

#[test]
fn src_004_html_config_discovers_at_least_ten() {
    let doc = make_doc(CONFIG_HTML_PAGE, "text/html");
    let source = SourceSpec {
        selectors: Some(HtmlSelectors {
            list: ".event-list".into(),
            list_link: "a".into(),
            detail_title: "h1".into(),
            detail_date: ".date".into(),
            ..Default::default()
        }),
        ..make_source("src-004", AdapterKind::HtmlConfig, SourceKind::Other)
    };
    let discovered = HtmlConfigAdapter
        .discover(&doc, &source)
        .expect("configured HTML page should parse");
    assert!(
        discovered.len() >= 10,
        "expected >=10 stubs, got {}",
        discovered.len()
    );
}

// ===========================================================================
// SRC-005: Generic HTML adapter
// ===========================================================================

#[test]
fn src_005_html_generic_filters_nav_and_finds_events() {
    let doc = make_doc(GENERIC_HTML_PAGE, "text/html");
    let source = make_source("src-005", AdapterKind::HtmlGeneric, SourceKind::Other);
    let discovered = HtmlGenericAdapter
        .discover(&doc, &source)
        .expect("generic HTML page should parse");
    assert!(
        discovered.len() >= 5,
        "expected >=5 event stubs, got {}",
        discovered.len()
    );
    // Precision >=90%: no navigation links promoted to EventStub.
    const NAV_TEXTS: &[&str] = &["home", "about", "contact", "login", "top"];
    for stub in &discovered {
        let lower = stub.title.trim().to_ascii_lowercase();
        assert!(
            !NAV_TEXTS.contains(&lower.as_str()),
            "nav link '{}' should not be promoted to EventStub",
            stub.title
        );
    }
}

// ===========================================================================
// MED-001: YouTube video detection
// ===========================================================================

#[test]
fn med_001_youtube_video_detected() {
    let base = Url::parse("https://example.com/test").unwrap();
    let document = Html::parse_document(MEDIA_DETECTION);
    let media = helpers::detect_media(&document, &base);
    let has_youtube = media
        .iter()
        .any(|m| m.media_type == MediaType::Video && m.platform.as_deref() == Some("youtube"));
    assert!(
        has_youtube,
        "expected at least one YouTube Video media resource, got {media:?}"
    );
}

// ===========================================================================
// MED-002: PDF slides detection
// ===========================================================================

#[test]
fn med_002_pdf_slides_detected() {
    let base = Url::parse("https://example.com/test").unwrap();
    let document = Html::parse_document(MEDIA_DETECTION);
    let media = helpers::detect_media(&document, &base);
    let has_slides = media.iter().any(|m| m.media_type == MediaType::Slides);
    assert!(
        has_slides,
        "expected at least one Slides media resource, got {media:?}"
    );
}

// ===========================================================================
// MED-003: Access classification
// ===========================================================================

#[test]
fn med_003_access_registration_required() {
    let document = Html::parse_document(MEDIA_DETECTION);
    let access = helpers::classify_access(&document);
    assert_eq!(
        access,
        PublicAccess::RegistrationRequired,
        "expected RegistrationRequired from 'Register now' text"
    );
}

// ===========================================================================
// TALK-001: Speaker extraction from JSON-LD performer
// ===========================================================================

#[test]
fn talk_001_speaker_extraction_from_jsonld() {
    let doc = make_doc(JSONLD_PAGE, "text/html");
    let source = make_source("talk-001", AdapterKind::JsonLd, SourceKind::JsonLd);
    let stubs = JsonLdAdapter
        .discover(&doc, &source)
        .expect("JSON-LD page should parse");
    let first = stubs
        .into_iter()
        .next()
        .expect("at least one stub from JSON-LD");
    // Enrich with the same document (acts as both discover and detail page).
    let candidate = JsonLdAdapter
        .enrich(first, std::slice::from_ref(&doc), &source)
        .expect("enrich should succeed");
    assert!(
        !candidate.event.talks.is_empty(),
        "expected >=1 talk from performer, got {}",
        candidate.event.talks.len()
    );
    assert_eq!(
        candidate.event.talks[0].speaker[0].role,
        PersonRole::Speaker
    );
}

// ===========================================================================
// §67: XXE attack must not panic (RSS adapter is XXE-safe via feed-rs/quick-xml)
// ===========================================================================

#[test]
fn s67_xxe_attack_does_not_panic() {
    let doc = make_doc(XXE_ATTACK, "application/rss+xml");
    let source = make_source("s67-xxe", AdapterKind::Rss, SourceKind::RssFeed);
    let result = RssAdapter.discover(&doc, &source);
    match result {
        Ok(stubs) => {
            for s in &stubs {
                let title = s.title.to_ascii_lowercase();
                assert!(
                    !title.contains("root:") && !title.contains("/etc/passwd"),
                    "XXE entity was expanded into title: {:?}",
                    s.title
                );
            }
        }
        Err(AdapterError::Parse { .. }) => {}
        Err(other) => panic!("expected Ok or Parse error, got {other:?}"),
    }
}

// ===========================================================================
// §67: ICS depth guard rejects >32 BEGIN: lines without parsing
// ===========================================================================

#[test]
fn s67_ics_depth_guard_rejects_deep_nesting() {
    let doc = make_doc(DEEP_NESTED_ICS, "text/calendar");
    let source = make_source("s67-depth", AdapterKind::Ics, SourceKind::IcsFeed);
    let result = IcsAdapter.discover(&doc, &source);
    assert!(
        matches!(result, Err(AdapterError::Parse { .. })),
        "deep nested ICS should return Err(Parse), got {result:?}"
    );
}

// ===========================================================================
// §67: Malformed (truncated) ICS must not panic
// ===========================================================================

#[test]
fn s67_malformed_ics_does_not_panic() {
    let doc = make_doc(MALFORMED_ICS, "text/calendar");
    let source = make_source("s67-malformed", AdapterKind::Ics, SourceKind::IcsFeed);
    let result = IcsAdapter.discover(&doc, &source);
    match result {
        Ok(stubs) => {
            assert!(
                stubs.is_empty(),
                "truncated ICS should yield no stubs, got {stubs:?}"
            );
        }
        Err(AdapterError::Parse { .. }) => {}
        Err(other) => panic!("expected Ok or Parse error, got {other:?}"),
    }
}
