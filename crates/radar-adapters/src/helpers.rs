//! Shared adapter helpers (M2 Todo 3). Pure parsing utilities used by all
//! Wave 2 adapters: RSS, ICS, JSON-LD, HTML config, HTML generic.

pub use scraper::Html;
use scraper::Selector;
use url::Url;

use radar_core::{
    EventType, MediaId, MediaResource, MediaType, PublicAccess, SourceEvidence, contains_phrase,
    deterministic_id, normalize_text,
};

// ===========================================================================
// HtmlFields + extract_html_fields
// ===========================================================================

/// Fields extracted from an HTML page by the generic HTML heuristic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HtmlFields {
    pub title: Option<String>,
    pub description: Option<String>,
    pub date_text: Option<String>,
    pub location_text: Option<String>,
}

/// Extract title, description, date, and location text from an HTML page using
/// simple heuristics. Returns all-`None` for empty or malformed input.
pub fn extract_html_fields(document: &Html, _base_url: &Url) -> HtmlFields {
    HtmlFields {
        title: extract_title(document),
        description: extract_description(document),
        date_text: extract_date_text(document),
        location_text: extract_location_text(document),
    }
}

fn extract_title(document: &Html) -> Option<String> {
    if let Some(text) = select_first_text(document, "h1") {
        return Some(text);
    }
    if let Some(text) = select_first_text(document, "title") {
        return Some(text);
    }
    select_meta_content(document, "property", "og:title")
}

fn extract_description(document: &Html) -> Option<String> {
    if let Some(content) = select_meta_content(document, "name", "description") {
        return Some(content);
    }
    if let Some(content) = select_meta_content(document, "property", "og:description") {
        return Some(content);
    }
    let selector = Selector::parse("p").ok()?;
    for element in document.select(&selector) {
        let text = clean_text(&element.text().collect::<String>());
        if text.chars().count() > 20 {
            return Some(text);
        }
    }
    None
}

fn extract_date_text(document: &Html) -> Option<String> {
    if let Ok(selector) = Selector::parse("time") {
        for element in document.select(&selector) {
            if let Some(dt) = element.attr("datetime") {
                let cleaned = clean_text(dt);
                if !cleaned.is_empty() {
                    return Some(cleaned);
                }
            }
            let text = clean_text(&element.text().collect::<String>());
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    if let Ok(selector) = Selector::parse(
        r#"[class*="date"], [id*="date"], [class*="time"], [id*="time"], [class*="when"], [id*="when"]"#,
    ) {
        for element in document.select(&selector) {
            let text = clean_text(&element.text().collect::<String>());
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn extract_location_text(document: &Html) -> Option<String> {
    if let Ok(selector) = Selector::parse(
        r#"[class*="location"], [id*="location"], [class*="venue"], [id*="venue"], [class*="place"], [id*="place"], [class*="address"], [id*="address"]"#,
    ) {
        for element in document.select(&selector) {
            let text = clean_text(&element.text().collect::<String>());
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn select_first_text(document: &Html, selector_str: &str) -> Option<String> {
    let selector = Selector::parse(selector_str).ok()?;
    let element = document.select(&selector).next()?;
    let text = clean_text(&element.text().collect::<String>());
    if text.is_empty() { None } else { Some(text) }
}

fn select_meta_content(document: &Html, attr: &str, value: &str) -> Option<String> {
    let selector_str = format!("meta[{}=\"{}\"]", attr, value);
    let selector = Selector::parse(&selector_str).ok()?;
    let content = document.select(&selector).next()?.attr("content")?;
    let cleaned = clean_text(content);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ===========================================================================
// detect_media
// ===========================================================================

/// Detect media resources (video, slides, PDFs) embedded in an HTML page.
pub fn detect_media(document: &Html, base_url: &Url) -> Vec<MediaResource> {
    let mut results = Vec::new();

    if let Ok(selector) = Selector::parse("a") {
        for element in document.select(&selector) {
            if let Some(href) = element.attr("href")
                && let Ok(resolved) = base_url.join(href)
            {
                let link_text = element.text().collect::<String>();
                let title_attr = element.attr("title");
                if let Some(media) = classify_link(&resolved, &link_text, title_attr, base_url) {
                    results.push(media);
                }
            }
        }
    }

    if let Ok(selector) = Selector::parse("iframe") {
        for element in document.select(&selector) {
            if let Some(src) = element.attr("src")
                && let Ok(resolved) = base_url.join(src)
            {
                results.push(make_video(&resolved, base_url));
            }
        }
    }

    if let Ok(selector) = Selector::parse("video") {
        for element in document.select(&selector) {
            if let Some(src) = element.attr("src")
                && let Ok(resolved) = base_url.join(src)
            {
                results.push(make_video(&resolved, base_url));
            }
        }
    }

    results
}

fn classify_link(
    url: &Url,
    link_text: &str,
    title_attr: Option<&str>,
    base_url: &Url,
) -> Option<MediaResource> {
    if let Some(platform) = classify_video_platform(url) {
        return Some(MediaResource {
            id: MediaId(deterministic_id(&[url.as_str()])),
            media_type: MediaType::Video,
            title: None,
            url: url.clone(),
            platform: Some(platform.into()),
            public_access: PublicAccess::Unknown,
            published_at: None,
            source: make_source_evidence(base_url),
        });
    }
    if is_pdf(url) {
        let mut context = String::from(link_text);
        context.push(' ');
        if let Some(t) = title_attr {
            context.push_str(t);
            context.push(' ');
        }
        context.push_str(url.path());
        let context = context.to_lowercase();
        let media_type = if context.contains("slides")
            || context.contains("presentation")
            || context.contains("handout")
        {
            MediaType::Slides
        } else if context.contains("program") {
            MediaType::ProgramPdf
        } else if context.contains("abstract") {
            MediaType::AbstractPdf
        } else {
            MediaType::Other
        };
        return Some(MediaResource {
            id: MediaId(deterministic_id(&[url.as_str()])),
            media_type,
            title: None,
            url: url.clone(),
            platform: None,
            public_access: PublicAccess::Unknown,
            published_at: None,
            source: make_source_evidence(base_url),
        });
    }
    None
}

fn make_video(url: &Url, base_url: &Url) -> MediaResource {
    MediaResource {
        id: MediaId(deterministic_id(&[url.as_str()])),
        media_type: MediaType::Video,
        title: None,
        url: url.clone(),
        platform: classify_video_platform(url).map(|s| s.into()),
        public_access: PublicAccess::Unknown,
        published_at: None,
        source: make_source_evidence(base_url),
    }
}

fn classify_video_platform(url: &Url) -> Option<&'static str> {
    let s = url.as_str();
    if s.contains("youtube.com/watch") || s.contains("youtu.be/") || s.contains("youtube.com/embed")
    {
        Some("youtube")
    } else if s.contains("vimeo.com/") {
        Some("vimeo")
    } else if s.contains("bilibili.com/video/") {
        Some("bilibili")
    } else {
        None
    }
}

fn is_pdf(url: &Url) -> bool {
    url.path().to_lowercase().ends_with(".pdf")
}

fn make_source_evidence(base_url: &Url) -> SourceEvidence {
    SourceEvidence {
        source_id: String::new(),
        source_url: base_url.clone(),
        evidence: None,
        captured_at: None,
        native_id: None,
    }
}

// ===========================================================================
// classify_access
// ===========================================================================

/// Conservatively classify the public access level from an HTML document's
/// visible text and meta tags. Uses word-boundary matching to avoid
/// false positives like "free" inside "freedom" or "sso" inside a longer word.
pub fn classify_access(document: &Html) -> PublicAccess {
    let mut text = String::new();
    for t in document.root_element().text() {
        text.push_str(t);
        text.push(' ');
    }
    if let Ok(selector) = Selector::parse("meta") {
        for element in document.select(&selector) {
            if let Some(content) = element.attr("content") {
                text.push_str(content);
                text.push(' ');
            }
        }
    }
    let text = normalize_text(&text);

    const PAYWALLED: &[&str] = &[
        "subscription",
        "paywall",
        "paid",
        "fee required",
        "purchase required",
    ];
    const LOGIN: &[&str] = &[
        "login required",
        "sign in",
        "institutional access",
        "university login",
        "sso",
    ];
    const REGISTRATION: &[&str] = &["register", "registration required", "sign up", "rsvp"];
    const OPEN: &[&str] = &["free", "open access", "no registration", "public"];

    if PAYWALLED.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::Paywalled;
    }
    if LOGIN.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::InstitutionLogin;
    }
    if REGISTRATION.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::RegistrationRequired;
    }
    if OPEN.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::Open;
    }
    PublicAccess::Unknown
}

// ===========================================================================
// detect_event_type
// ===========================================================================

/// Detect the event type from free text using case-insensitive keyword matching.
pub fn detect_event_type(text: &str) -> EventType {
    let normalized = normalize_text(text);
    if normalized.is_empty() {
        return EventType::Unknown;
    }
    // Specific variants before generic ones.
    if normalized.contains("memorial conference") || normalized.contains("memorial meeting") {
        return EventType::MemorialConference;
    }
    if normalized.contains("award lecture")
        || normalized.contains("prize lecture")
        || normalized.contains("memorial lecture")
    {
        return EventType::AwardLecture;
    }
    if normalized.contains("summer school")
        || normalized.contains("winter school")
        || normalized.contains("spring school")
    {
        return EventType::SummerSchool;
    }
    if normalized.contains("distinguished lecture") || normalized.contains("plenary lecture") {
        return EventType::DistinguishedLecture;
    }
    if normalized.contains("public lecture") || normalized.contains("public talk") {
        return EventType::PublicLecture;
    }
    if normalized.contains("lecture series") || normalized.contains("lecture programme") {
        return EventType::LectureSeries;
    }
    if normalized.contains("research program")
        || normalized.contains("research programme")
        || normalized.contains("thematic program")
        || normalized.contains("semester program")
    {
        return EventType::ResearchProgram;
    }
    if normalized.contains("mini course")
        || normalized.contains("minicourse")
        || normalized.contains("short course")
    {
        return EventType::MiniCourse;
    }
    if normalized.contains("workshop") {
        return EventType::Workshop;
    }
    if normalized.contains("colloquium") {
        return EventType::Colloquium;
    }
    if normalized.contains("panel") {
        return EventType::Panel;
    }
    if normalized.contains("seminar") {
        return EventType::Seminar;
    }
    if normalized.contains("conference")
        || normalized.contains("meeting")
        || normalized.contains("symposium")
    {
        return EventType::Conference;
    }
    EventType::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn base() -> Url {
        Url::parse("https://example.com/").unwrap()
    }

    fn doc(html: &str) -> Html {
        Html::parse_document(html)
    }

    // --- extract_html_fields ---

    #[test]
    fn html_fields_basic() {
        let html = r#"<!DOCTYPE html>
        <html>
        <head>
            <title>Test Page</title>
            <meta name="description" content="A test description">
        </head>
        <body>
            <h1>Real Title</h1>
            <time datetime="2026-08-08">August 8, 2026</time>
            <p class="location">Berlin, Germany</p>
        </body>
        </html>"#;
        let fields = extract_html_fields(&doc(html), &base());
        assert_eq!(fields.title.as_deref(), Some("Real Title"));
        assert_eq!(fields.description.as_deref(), Some("A test description"));
        assert_eq!(fields.date_text.as_deref(), Some("2026-08-08"));
        assert_eq!(fields.location_text.as_deref(), Some("Berlin, Germany"));
    }

    #[test]
    fn html_fields_empty() {
        let fields = extract_html_fields(&doc(""), &base());
        assert!(fields.title.is_none());
        assert!(fields.description.is_none());
        assert!(fields.date_text.is_none());
        assert!(fields.location_text.is_none());
    }

    #[test]
    fn html_fields_malformed_no_panic() {
        let html = "<<<>><html><head><body>broken";
        let fields = extract_html_fields(&doc(html), &base());
        let _ = fields;
    }

    #[test]
    fn html_fields_title_fallbacks() {
        let html = "<html><head><title>Title Tag</title></head><body></body></html>";
        let fields = extract_html_fields(&doc(html), &base());
        assert_eq!(fields.title.as_deref(), Some("Title Tag"));

        let html = r#"<html><head><meta property="og:title" content="OG Title"></head><body></body></html>"#;
        let fields = extract_html_fields(&doc(html), &base());
        assert_eq!(fields.title.as_deref(), Some("OG Title"));
    }

    #[test]
    fn html_fields_description_fallback_to_p() {
        let html = r#"<html><body><p>short</p><p>This is a longer paragraph with more than twenty characters.</p></body></html>"#;
        let fields = extract_html_fields(&doc(html), &base());
        assert_eq!(
            fields.description.as_deref(),
            Some("This is a longer paragraph with more than twenty characters.")
        );
    }

    #[test]
    fn html_fields_date_from_class() {
        let html = r#"<html><body><div class="event-date">August 8, 2026</div></body></html>"#;
        let fields = extract_html_fields(&doc(html), &base());
        assert_eq!(fields.date_text.as_deref(), Some("August 8, 2026"));
    }

    // --- detect_media ---

    #[test]
    fn media_youtube_link() {
        let html = r#"<a href="https://www.youtube.com/watch?v=abc123">Watch</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    #[test]
    fn media_pdf_slides() {
        let html = r#"<a href="https://example.com/slides.pdf">Download slides</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Slides);
    }

    #[test]
    fn media_pdf_program() {
        let html = r#"<a href="https://example.com/program.pdf">Program</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::ProgramPdf);
    }

    #[test]
    fn media_pdf_abstract() {
        let html = r#"<a href="https://example.com/abstract.pdf">Abstract</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::AbstractPdf);
    }

    #[test]
    fn media_pdf_other() {
        let html = r#"<a href="https://example.com/paper.pdf">Paper</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Other);
    }

    #[test]
    fn media_empty() {
        let media = detect_media(&doc(""), &base());
        assert!(media.is_empty());
    }

    #[test]
    fn media_vimeo() {
        let html = r#"<a href="https://vimeo.com/12345">Video</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("vimeo"));
    }

    #[test]
    fn media_bilibili() {
        let html = r#"<a href="https://www.bilibili.com/video/BV1234">Video</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("bilibili"));
    }

    #[test]
    fn media_iframe_video() {
        let html = r#"<iframe src="https://example.com/embed"></iframe>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert!(media[0].platform.is_none());
    }

    #[test]
    fn media_relative_url() {
        let html = r#"<a href="/talks/video.pdf">slides</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].url.as_str(), "https://example.com/talks/video.pdf");
        assert_eq!(media[0].media_type, MediaType::Slides);
    }

    #[test]
    fn media_youtu_be() {
        let html = r#"<a href="https://youtu.be/abc123">Short link</a>"#;
        let media = detect_media(&doc(html), &base());
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    #[test]
    fn media_malformed_no_panic() {
        let html = "<<<>><a href=>broken</a>";
        let media = detect_media(&doc(html), &base());
        let _ = media;
    }

    // --- classify_access ---

    #[test]
    fn access_register() {
        let html = "<html><body>Please register now</body></html>";
        assert_eq!(
            classify_access(&doc(html)),
            PublicAccess::RegistrationRequired
        );
    }

    #[test]
    fn access_login() {
        let html = "<html><body>login required to view</body></html>";
        assert_eq!(classify_access(&doc(html)), PublicAccess::InstitutionLogin);
    }

    #[test]
    fn access_empty() {
        assert_eq!(classify_access(&doc("")), PublicAccess::Unknown);
    }

    #[test]
    fn access_conflicting_paywall_wins() {
        let html =
            "<html><body>Free but registration required and subscription needed</body></html>";
        assert_eq!(classify_access(&doc(html)), PublicAccess::Paywalled);
    }

    #[test]
    fn access_open() {
        let html = "<html><body>This event is free and open access</body></html>";
        assert_eq!(classify_access(&doc(html)), PublicAccess::Open);
    }

    #[test]
    fn access_registration_over_open() {
        let html = "<html><body>Free but please register</body></html>";
        assert_eq!(
            classify_access(&doc(html)),
            PublicAccess::RegistrationRequired
        );
    }

    #[test]
    fn access_login_over_registration() {
        let html = "<html><body>Please register. SSO login required.</body></html>";
        assert_eq!(classify_access(&doc(html)), PublicAccess::InstitutionLogin);
    }

    #[test]
    fn access_malformed_no_panic() {
        let html = "<<<>>broken<<";
        let result = classify_access(&doc(html));
        let _ = result;
    }

    // --- detect_event_type: all 14 variants ---

    #[test]
    fn event_type_conference() {
        assert_eq!(
            detect_event_type("Conference on Algebra"),
            EventType::Conference
        );
    }

    #[test]
    fn event_type_workshop() {
        assert_eq!(
            detect_event_type("Workshop on Graph Theory"),
            EventType::Workshop
        );
    }

    #[test]
    fn event_type_research_program() {
        assert_eq!(
            detect_event_type("Thematic Program on Random Graphs"),
            EventType::ResearchProgram
        );
    }

    #[test]
    fn event_type_public_lecture() {
        assert_eq!(
            detect_event_type("Public Lecture by Prof. X"),
            EventType::PublicLecture
        );
    }

    #[test]
    fn event_type_distinguished_lecture() {
        assert_eq!(
            detect_event_type("Distinguished Lecture Series"),
            EventType::DistinguishedLecture
        );
    }

    #[test]
    fn event_type_lecture_series() {
        assert_eq!(
            detect_event_type("Lecture Series on Number Theory"),
            EventType::LectureSeries
        );
    }

    #[test]
    fn event_type_summer_school() {
        assert_eq!(
            detect_event_type("Summer School on Topology"),
            EventType::SummerSchool
        );
    }

    #[test]
    fn event_type_mini_course() {
        assert_eq!(
            detect_event_type("Mini Course on Category Theory"),
            EventType::MiniCourse
        );
    }

    #[test]
    fn event_type_colloquium() {
        assert_eq!(detect_event_type("Colloquium Talk"), EventType::Colloquium);
    }

    #[test]
    fn event_type_panel() {
        assert_eq!(
            detect_event_type("Panel Discussion on Math Education"),
            EventType::Panel
        );
    }

    #[test]
    fn event_type_award_lecture() {
        assert_eq!(
            detect_event_type("Award Lecture in Honor of Prof. X"),
            EventType::AwardLecture
        );
    }

    #[test]
    fn event_type_memorial_conference() {
        assert_eq!(
            detect_event_type("Memorial Conference for John Doe"),
            EventType::MemorialConference
        );
    }

    #[test]
    fn event_type_seminar() {
        assert_eq!(
            detect_event_type("Seminar on Algebraic Geometry"),
            EventType::Seminar
        );
    }

    #[test]
    fn event_type_unknown() {
        assert_eq!(detect_event_type("Some random event"), EventType::Unknown);
    }

    #[test]
    fn event_type_empty() {
        assert_eq!(detect_event_type(""), EventType::Unknown);
    }

    // --- detect_event_type: priority / specificity ---

    #[test]
    fn event_type_memorial_conference_before_conference() {
        assert_eq!(
            detect_event_type("Memorial Conference on Geometry"),
            EventType::MemorialConference
        );
    }

    #[test]
    fn event_type_memorial_meeting_before_conference() {
        assert_eq!(
            detect_event_type("Memorial Meeting for John"),
            EventType::MemorialConference
        );
    }

    #[test]
    fn event_type_memorial_lecture_is_award_lecture() {
        assert_eq!(
            detect_event_type("Memorial Lecture for John"),
            EventType::AwardLecture
        );
    }

    #[test]
    fn event_type_award_lecture_before_lecture_series() {
        assert_eq!(
            detect_event_type("Award Lecture Series"),
            EventType::AwardLecture
        );
    }

    #[test]
    fn event_type_summer_school_not_just_school() {
        assert_eq!(
            detect_event_type("Summer School on Topology"),
            EventType::SummerSchool
        );
    }

    #[test]
    fn event_type_case_insensitive() {
        assert_eq!(
            detect_event_type("SUMMER SCHOOL on TOPOLOGY"),
            EventType::SummerSchool
        );
    }

    #[test]
    fn event_type_collapses_whitespace() {
        assert_eq!(
            detect_event_type("Summer   School   on   Topology"),
            EventType::SummerSchool
        );
    }
}
