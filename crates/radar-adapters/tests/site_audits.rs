//! M6 site-audit golden tests (§45). Verifies each enabled source's sanitized
//! fixture parses with the declared adapter kind and produces ≥1 event stub.
//! Fixtures are sanitized offline copies (§45); no real-website data is sent
//! over the network.

use radar_adapters::html_config::HtmlConfigAdapter;
use radar_adapters::html_generic::HtmlGenericAdapter;
use radar_adapters::jsonld::JsonLdAdapter;
use radar_adapters::rss::RssAdapter;
use radar_core::config::HtmlSelectors;
use radar_core::date::DatePrecision;
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

// HTML-config fixtures: discover with the site-specific selectors shipped in
// `config/sources.toml` (§P-5). Two tiers:
//   - Group A (tuned selectors): assert ≥1 stub AND that titles look like real
//     events (not nav links like "Home" / "Skip to content").
//   - Group B (permissive fallback): assert ≥1 stub only — their fixtures are
//     landing pages without event lists; selector tuning needs a re-captured
//     fixture from the correct event-list URL.
fn source_from_embedded(id: &str) -> SourceSpec {
    let config = radar_core::SourcesConfig::embedded().expect("embedded sources.toml parses");
    let s = config
        .sources
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("source {id} must be in embedded sources.toml"));
    SourceSpec {
        entrypoint: s.entrypoint.clone(),
        selectors: s.selectors.clone(),
        ..make_source(id, AdapterKind::HtmlConfig)
    }
}

fn discover_fixture(id: &str, fixture_path: &str) -> Vec<EventStub> {
    let body = std::fs::read_to_string(fixture_path)
        .unwrap_or_else(|e| panic!("{id}: fixture {fixture_path}: {e}"));
    let source = source_from_embedded(id);
    let url = source
        .entrypoint
        .as_ref()
        .map(|u| u.as_str())
        .unwrap_or("https://example.com/");
    let doc = make_doc(&body, "text/html", url);
    HtmlConfigAdapter
        .discover(&doc, &source)
        .unwrap_or_else(|e| panic!("{id}: discover must not error, got {e:?}"))
}

const NAV_LINK_TITLES: &[&str] = &[
    "Home",
    "About",
    "Search",
    "Menu",
    "Skip to content",
    "Skip to main content",
    "Contact",
    "Site Map",
    "Imprint",
    "Privacy Policy",
    "Website",
];

fn looks_like_nav_link(title: &str) -> bool {
    NAV_LINK_TITLES
        .iter()
        .any(|nav| title.eq_ignore_ascii_case(nav))
}

fn assert_real_events(id: &str, stubs: &[EventStub], min_count: usize) {
    assert!(
        stubs.len() >= min_count,
        "{id}: expected >={min_count} stubs, got {}",
        stubs.len()
    );
    let nav_like = stubs
        .iter()
        .filter(|s| looks_like_nav_link(&s.title))
        .count();
    assert!(
        nav_like < stubs.len(),
        "{id}: all {min_count}+ stubs look like nav links (titles: {:?})",
        stubs.iter().take(5).map(|s| &s.title).collect::<Vec<_>>()
    );
}

// Group A — tuned selectors (§P-5). These fixtures contain real event lists.

#[test]
fn site_mpim_html_config_discovers_real_events() {
    let stubs = discover_fixture("mpim", "tests/fixtures/sites/mpim-list.html");
    assert_real_events("mpim", &stubs, 10);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("primes") || s.title.contains("L-functions")),
        "mpim: expected a real seminar title, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
}

#[test]
fn site_cambridge_dpmms_html_config_discovers_real_events() {
    let stubs = discover_fixture(
        "cambridge-dpmms",
        "tests/fixtures/sites/cambridge-dpmms-list.html",
    );
    assert_real_events("cambridge-dpmms", &stubs, 6);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Statistics Clinic") || s.title.contains("Particles")),
        "cambridge-dpmms: expected real seminar titles, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
}

#[test]
fn site_oxford_math_html_config_discovers_real_events() {
    let stubs = discover_fixture("oxford-math", "tests/fixtures/sites/oxford-math-list.html");
    assert_real_events("oxford-math", &stubs, 1);
    assert!(
        stubs.iter().any(|s| s.title.contains("Sarah Hart")),
        "oxford-math: expected the Sarah Hart event, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
}

#[test]
fn site_ams_calendar_html_config_discovers_real_events() {
    let stubs = discover_fixture(
        "ams-calendar",
        "tests/fixtures/sites/ams-calendar-list.html",
    );
    assert_real_events("ams-calendar", &stubs, 90);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Conference") || s.title.contains("Summer School")),
        "ams-calendar: expected conference/school titles, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
    // ams-calendar `dt.event_dates` contains the date text alongside child
    // `<a>` links ("Expand to view...", "Download .ics"). Without `direct_text`
    // filtering, `element.text()` pollutes the date string and 0% of stubs
    // carry a date_hint.
    let dated = stubs
        .iter()
        .filter(|s| {
            s.date_hint
                .as_ref()
                .map(|d| d.precision != DatePrecision::Unknown)
                .unwrap_or(false)
        })
        .count();
    assert!(
        dated > 0,
        "ams-calendar: expected >0 stubs with dated date_hint, got {dated}/{}",
        stubs.len()
    );
}

#[test]
fn site_princeton_math_html_config_discovers_real_events() {
    let stubs = discover_fixture(
        "princeton-math",
        "tests/fixtures/sites/princeton-math-list.html",
    );
    assert_real_events("princeton-math", &stubs, 11);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Positive Mass Theorem")),
        "princeton-math: expected the Positive Mass Theorem talk, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
    assert!(
        stubs.iter().all(|s| s.date_hint.is_some()),
        "princeton-math: every stub should carry a date_hint from <time datetime>"
    );
}

#[test]
fn site_mit_math_html_config_discovers_real_events() {
    let stubs = discover_fixture("mit-math", "tests/fixtures/sites/mit-math-list.html");
    assert_real_events("mit-math", &stubs, 40);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Gross--Zagier") || s.title.contains("Representation Theory")),
        "mit-math: expected Gross-Zagier or Representation Theory, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
    assert!(
        stubs.iter().filter(|s| s.date_hint.is_some()).count() >= 30,
        "mit-math: expected >=30 stubs with date_hint, got {}",
        stubs.iter().filter(|s| s.date_hint.is_some()).count()
    );
}

// Group B — re-captured fixtures with tuned selectors (§P-5). These sites had
// landing-page fixtures that were re-captured from the correct event-list URL.

#[test]
fn site_fields_html_config_discovers_real_events() {
    let stubs = discover_fixture("fields", "tests/fixtures/sites/fields-list.html");
    assert_real_events("fields", &stubs, 2);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Complex Analysis") || s.title.contains("Mathematical AI")),
        "fields: expected Complex Analysis or Mathematical AI Seminar, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
}

#[test]
fn site_ini_html_config_discovers_real_events() {
    let stubs = discover_fixture("ini", "tests/fixtures/sites/newton-list.html");
    assert_real_events("ini", &stubs, 20);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Koopman") || s.title.contains("scattering")),
        "ini: expected Koopman or scattering seminar, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
}

#[test]
fn site_hcm_html_config_discovers_real_events() {
    let stubs = discover_fixture("hcm", "tests/fixtures/sites/hcm-list.html");
    assert_real_events("hcm", &stubs, 1000);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Floer") || s.title.contains("resonances")),
        "hcm: expected Floer or resonances seminar, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
    // hcm list-page dates are year-less ("Nov 21", "Jan 01 - Dec 30") and
    // require the year-hint path in `parse_date_with_year_hint`. Without it,
    // 0% of stubs carry a date_hint.
    let dated = stubs
        .iter()
        .filter(|s| {
            s.date_hint
                .as_ref()
                .map(|d| d.precision != DatePrecision::Unknown)
                .unwrap_or(false)
        })
        .count();
    assert!(
        dated > 0,
        "hcm: expected >0 stubs with dated date_hint, got {dated}/{}",
        stubs.len()
    );
}

// Group C — tuned selectors against re-captured fixtures. eth-math and icm
// have non-standard event structures (series-page links / Cvent article blocks)
// that were re-captured from the correct event-list URL and tuned to target
// the content area, avoiding nav-link garbage.

#[test]
fn site_eth_math_html_config_discovers_real_events() {
    let stubs = discover_fixture("eth-math", "tests/fixtures/sites/eth-math-list.html");
    assert_real_events("eth-math", &stubs, 1);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Algebraic Geometry") || s.title.contains("Colloquium")),
        "eth-math: expected Algebraic Geometry or Colloquium series, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
    );
}

#[test]
fn site_icm_html_config_discovers_real_events() {
    let stubs = discover_fixture("icm", "tests/fixtures/sites/icm-list.html");
    assert_real_events("icm", &stubs, 5);
    assert!(
        stubs
            .iter()
            .any(|s| s.title.contains("Hilbert") || s.title.contains("ICM 2026")),
        "icm: expected Hilbert or ICM 2026 article, got {:?}",
        stubs.iter().take(3).map(|s| &s.title).collect::<Vec<_>>()
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
