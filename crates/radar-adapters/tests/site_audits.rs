//! M6 site-audit golden tests (§45). Verifies each enabled source's sanitized
//! fixture parses with the declared adapter kind and produces ≥1 event stub.
//! Fixtures are sanitized offline copies (§45); no real-website data is sent
//! over the network.

use radar_adapters::html_config::HtmlConfigAdapter;
use radar_adapters::html_generic::HtmlGenericAdapter;
use radar_adapters::jsonld::JsonLdAdapter;
use radar_adapters::rss::RssAdapter;
use radar_core::config::HtmlSelectors;
use radar_core::{
    AdapterKind, EventStub, FetchedDocument, SourceAdapter, SourceKind, SourceSpec, SourceTier,
};
use url::Url;

const CLAY_FEED: &str = include_str!("fixtures/sites/clay-list.xml");
const IHES_FEED: &str = include_str!("fixtures/sites/ihes-list.xml");
const STANFORD_HTML: &str = include_str!("fixtures/sites/stanford-math-list.html");

fn make_doc(body: &str, content_type: &str, url: &str) -> FetchedDocument {
    FetchedDocument {
        url: Url::parse(url).unwrap(),
        final_url: Url::parse(url).unwrap(),
        status: 200,
        content_type: Some(content_type.into()),
        body: body.as_bytes().to_vec(),
        fetched_at: std::time::SystemTime::now().into(),
    }
}

fn make_source(id: &str, adapter: AdapterKind) -> SourceSpec {
    SourceSpec {
        id: id.to_string(),
        name: id.to_string(),
        tier: SourceTier::A,
        kind: SourceKind::InstitutionCalendar,
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

// clay: RSS events feed, ≥10 items.
#[test]
fn site_clay_rss_discovers_events() {
    let doc = make_doc(
        CLAY_FEED,
        "application/rss+xml",
        "https://www.claymath.org/events/feed/",
    );
    let source = make_source("clay", AdapterKind::Rss);
    let stubs = RssAdapter
        .discover(&doc, &source)
        .expect("clay RSS fixture must parse");
    assert!(
        stubs.len() >= 10,
        "clay fixture expected >=10 stubs, got {}",
        stubs.len()
    );
    assert!(
        stubs.iter().all(|s| !s.title.is_empty()),
        "all clay stubs must have non-empty titles"
    );
}

// ihes: RSS event-category feed, ≥10 items.
#[test]
fn site_ihes_rss_discovers_events() {
    let doc = make_doc(
        IHES_FEED,
        "application/rss+xml",
        "https://www.ihes.fr/en/category/event/feed/",
    );
    let source = make_source("ihes", AdapterKind::Rss);
    let stubs = RssAdapter
        .discover(&doc, &source)
        .expect("ihes RSS fixture must parse");
    assert!(
        stubs.len() >= 10,
        "ihes fixture expected >=10 stubs, got {}",
        stubs.len()
    );
}

// stanford: JSON-LD Event blocks, ≥10 stubs.
#[test]
fn site_stanford_jsonld_discovers_events() {
    let doc = make_doc(STANFORD_HTML, "text/html", "https://events.stanford.edu/");
    let source = make_source("stanford-math", AdapterKind::JsonLd);
    let stubs = JsonLdAdapter
        .discover(&doc, &source)
        .expect("stanford JSON-LD fixture must parse");
    assert!(
        stubs.len() >= 10,
        "stanford fixture expected >=10 stubs, got {}",
        stubs.len()
    );
    assert!(
        stubs.iter().all(|s| !s.title.is_empty()),
        "all stanford stubs must have non-empty titles"
    );
}

// HTML-config fixtures: parse with a permissive selector (body → all links)
// and verify ≥1 stub. These sites use varied markup; site-specific selectors
// land when each source is individually wired (§P-5). The golden assertion
// is "the fixture is parseable and yields ≥1 candidate link."
fn html_fixture_parses(path: &str, url: &str, id: &str) {
    let body = std::fs::read_to_string(path).expect("fixture file exists");
    let doc = make_doc(&body, "text/html", url);
    let source = SourceSpec {
        selectors: Some(HtmlSelectors {
            list: "body".into(),
            list_link: "a".into(),
            detail_title: "h1".into(),
            detail_date: "time".into(),
            ..Default::default()
        }),
        ..make_source(id, AdapterKind::HtmlConfig)
    };
    let stubs = HtmlConfigAdapter
        .discover(&doc, &source)
        .expect("HTML-config fixture must not error");
    assert!(!stubs.is_empty(), "{id}: fixture expected >=1 stub, got 0");
}

#[test]
fn site_fields_html_config_parses() {
    html_fixture_parses(
        "tests/fixtures/sites/fields-list.html",
        "https://www.fields.utoronto.ca/activities",
        "fields",
    );
}

#[test]
fn site_mpim_html_config_parses() {
    html_fixture_parses(
        "tests/fixtures/sites/mpim-list.html",
        "https://www.mpim-bonn.mpg.de/calendar",
        "mpim",
    );
}

#[test]
fn site_princeton_html_config_parses() {
    html_fixture_parses(
        "tests/fixtures/sites/princeton-math-list.html",
        "https://www.math.princeton.edu/events",
        "princeton-math",
    );
}

#[test]
fn site_oxford_html_config_parses() {
    html_fixture_parses(
        "tests/fixtures/sites/oxford-math-list.html",
        "https://www.maths.ox.ac.uk/events",
        "oxford-math",
    );
}

#[test]
fn site_eth_html_config_parses() {
    html_fixture_parses(
        "tests/fixtures/sites/eth-math-list.html",
        "https://math.ethz.ch/news-and-events/events.html",
        "eth-math",
    );
}

// ===========================================================================
// §18 media coverage: ≥3 adapter kinds must produce MediaResource entries
// when enriching a detail page with media links (output-based definition).
// ===========================================================================

fn stub(title: &str, url: &str, source_id: &str) -> EventStub {
    EventStub {
        title: title.into(),
        url: Url::parse(url).unwrap(),
        date_hint: None,
        source: radar_core::SourceEvidence {
            source_id: source_id.into(),
            source_url: Url::parse(url).unwrap(),
            evidence: None,
            captured_at: None,
            native_id: None,
        },
    }
}

#[test]
fn html_generic_enrich_extracts_youtube_media() {
    let detail = r#"<html><body>
        <h1>Talk on Random Graphs</h1>
        <time datetime="2026-09-15">Sep 15</time>
        <a href="https://www.youtube.com/watch?v=abc123">Watch recording</a>
    </body></html>"#;
    let doc = make_doc(detail, "text/html", "https://example.com/talk/1");
    let source = make_source("media-generic", AdapterKind::HtmlGeneric);
    let s = stub(
        "Talk on Random Graphs",
        "https://example.com/talk/1",
        "media-generic",
    );
    let candidate = HtmlGenericAdapter
        .enrich(s, std::slice::from_ref(&doc), &source)
        .expect("html_generic enrich must succeed");
    assert!(
        candidate
            .event
            .media
            .iter()
            .any(|m| m.media_type == radar_core::MediaType::Video),
        "html_generic must extract video media, got {:?}",
        candidate.event.media
    );
}

#[test]
fn html_config_enrich_extracts_slides_pdf() {
    let detail = r#"<html><body>
        <h1>Workshop on Topology</h1>
        <time datetime="2026-10-20">Oct 20</time>
        <a href="https://example.com/slides/topology.pdf">Download slides</a>
    </body></html>"#;
    let doc = make_doc(detail, "text/html", "https://example.com/workshop/1");
    let source = SourceSpec {
        selectors: Some(HtmlSelectors {
            list: "body".into(),
            list_link: "a".into(),
            detail_title: "h1".into(),
            detail_date: "time".into(),
            ..Default::default()
        }),
        ..make_source("media-config", AdapterKind::HtmlConfig)
    };
    let s = stub(
        "Workshop on Topology",
        "https://example.com/workshop/1",
        "media-config",
    );
    let candidate = HtmlConfigAdapter
        .enrich(s, std::slice::from_ref(&doc), &source)
        .expect("html_config enrich must succeed");
    assert!(
        candidate
            .event
            .media
            .iter()
            .any(|m| m.media_type == radar_core::MediaType::Slides),
        "html_config must extract Slides media, got {:?}",
        candidate.event.media
    );
}

#[test]
fn rss_enrich_extracts_vimeo_media_from_html_content() {
    let detail = r#"<html><body>
        <a href="https://vimeo.com/99988">View video</a>
    </body></html>"#;
    let doc = make_doc(detail, "text/html", "https://example.com/feed");
    let source = make_source("media-rss", AdapterKind::Rss);
    let s = stub(
        "Conference Talk",
        "https://example.com/feed/item/1",
        "media-rss",
    );
    let candidate = RssAdapter
        .enrich(s, std::slice::from_ref(&doc), &source)
        .expect("rss enrich must succeed");
    assert!(
        candidate
            .event
            .media
            .iter()
            .any(|m| m.media_type == radar_core::MediaType::Video),
        "rss must extract video media from HTML content, got {:?}",
        candidate.event.media
    );
}
