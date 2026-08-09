//! M6 site-audit golden tests (§45). Verifies each enabled source's sanitized
//! fixture parses with the declared adapter kind and produces ≥1 event stub.
//! Fixtures are sanitized offline copies (§45); no real-website data is sent
//! over the network.

use radar_adapters::html_config::HtmlConfigAdapter;
use radar_adapters::jsonld::JsonLdAdapter;
use radar_adapters::rss::RssAdapter;
use radar_core::config::HtmlSelectors;
use radar_core::{AdapterKind, FetchedDocument, SourceAdapter, SourceKind, SourceSpec, SourceTier};
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
