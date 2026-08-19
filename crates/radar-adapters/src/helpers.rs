//! Shared adapter helpers (M2 Todo 3). Pure parsing utilities used by all
//! Wave 2 adapters: RSS, ICS, JSON-LD, HTML config, HTML generic.

pub use scraper::Html;
use scraper::Selector;
use std::cell::RefCell;
use std::sync::OnceLock;
use url::Url;

pub(crate) fn cached_selector(selector_str: &'static str) -> Option<&'static Selector> {
    static SELECTORS: OnceLock<std::collections::HashMap<&'static str, Selector>> = OnceLock::new();
    let map = SELECTORS.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        let candidates: &[&'static str] = &[
            "p",
            "time",
            r#"[class*="date"], [id*="date"], [class*="time"], [id*="time"], [class*="when"], [id*="when"]"#,
            r#"[class*="location"], [id*="location"], [class*="venue"], [id*="venue"], [class*="place"], [id*="place"], [class*="address"], [id*="address"]"#,
            "a",
            "iframe",
            "video",
            "audio",
            "meta",
            "h1",
            "title",
            r#"script[type="application/ld+json"]"#,
        ];
        for &s in candidates {
            if let Ok(sel) = Selector::parse(s) {
                m.insert(s, sel);
            }
        }
        m
    });
    map.get(selector_str)
}

thread_local! {
    static RUNTIME_SELECTOR_CACHE: RefCell<std::collections::HashMap<String, Selector>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Parse `selector_str` with a per-thread cache (stored in
/// `RUNTIME_SELECTOR_CACHE`). On a cache hit, returns a clone of the previously
/// parsed [`Selector`]; on a miss, parses, caches, and returns the selector.
/// Avoids repeated CSS parsing on every `enrich` call (HCM: ~1000 events ×
/// 2–5 selectors per source).
pub(crate) fn cached_selector_runtime(selector_str: &str) -> Result<Selector, String> {
    RUNTIME_SELECTOR_CACHE.with(|cache| {
        if let Some(sel) = cache.borrow().get(selector_str).cloned() {
            return Ok(sel);
        }
        let sel = Selector::parse(selector_str).map_err(|e| e.to_string())?;
        cache
            .borrow_mut()
            .insert(selector_str.to_string(), sel.clone());
        Ok(sel)
    })
}

use radar_core::{
    AccessInfo, Event, EventDate, EventStatus, EventType, Location, MediaId, MediaResource,
    MediaType, PersonHit, PublicAccess, ScoreComponents, SourceEvidence, Talk, contains_phrase,
    deterministic_id, event_id, normalize_text,
};

/// Decode a document body as UTF-8 with U+FFFD replacement for invalid bytes.
/// Non-UTF-8 pages preserve their valid bytes instead of becoming empty
/// strings (§66 — parser cannot silently drop data on encoding errors).
pub(crate) fn doc_body<'a>(body: &'a [u8]) -> std::borrow::Cow<'a, str> {
    match std::str::from_utf8(body) {
        Ok(s) => std::borrow::Cow::Borrowed(s),
        Err(_) => std::borrow::Cow::Owned(
            body.utf8_chunks()
                .flat_map(|c| {
                    c.valid()
                        .chars()
                        .chain(std::iter::repeat_n('\u{FFFD}', c.invalid().len()))
                })
                .collect(),
        ),
    }
}

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
pub fn extract_html_fields(document: &Html) -> HtmlFields {
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
    let selector = cached_selector("p")?;
    for element in document.select(selector) {
        let text = clean_text(&element.text().collect::<String>());
        if text.chars().count() > 20 {
            return Some(text);
        }
    }
    None
}

fn extract_date_text(document: &Html) -> Option<String> {
    if let Some(selector) = cached_selector("time") {
        for element in document.select(selector) {
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
    if let Some(selector) = cached_selector(
        r#"[class*="date"], [id*="date"], [class*="time"], [id*="time"], [class*="when"], [id*="when"]"#,
    ) {
        for element in document.select(selector) {
            let text = clean_text(&element.text().collect::<String>());
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn extract_location_text(document: &Html) -> Option<String> {
    if let Some(selector) = cached_selector(
        r#"[class*="location"], [id*="location"], [class*="venue"], [id*="venue"], [class*="place"], [id*="place"], [class*="address"], [id*="address"]"#,
    ) {
        for element in document.select(selector) {
            let text = clean_text(&element.text().collect::<String>());
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn select_first_text(document: &Html, selector_str: &'static str) -> Option<String> {
    let selector = cached_selector(selector_str)?;
    let element = document.select(selector).next()?;
    let text = clean_text(&element.text().collect::<String>());
    if text.is_empty() { None } else { Some(text) }
}

fn select_meta_content(document: &Html, attr: &str, value: &str) -> Option<String> {
    let selector_str = format!("meta[{}=\"{}\"]", attr, value);
    let selector = cached_selector_runtime(&selector_str).ok()?;
    let content = document.select(&selector).next()?.attr("content")?;
    let cleaned = clean_text(content);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

pub(crate) fn clean_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for (i, word) in text.split_whitespace().enumerate() {
        if i > 0 {
            result.push(' ');
        }
        result.push_str(word);
    }
    result
}

/// M04: strip HTML tags from a string, returning plain text with normalized
/// whitespace. JSON-LD `description` fields often contain raw HTML
/// (`<p>...</p>`, `<a href=...>`, `<script>`) which must not be passed
/// through to the JSON output verbatim — downstream consumers render the
/// description as text, and raw HTML would either render unintentionally
/// or inject markup. Parses with `scraper` and collects text nodes, then
/// collapses whitespace via `clean_text`. Returns the input unchanged
/// (after whitespace normalization) when it contains no `<` — the common
/// case for plain-text descriptions — so the parse cost is avoided.
pub fn strip_html_to_text(html: &str) -> String {
    if !html.contains('<') {
        return clean_text(html);
    }
    let fragment = Html::parse_fragment(html);
    let text: String = fragment.root_element().text().collect();
    clean_text(&text)
}

// ===========================================================================
// detect_media
// ===========================================================================

/// Detect media resources (video, slides, PDFs) embedded in an HTML page.
/// Within-event duplicates (the same URL surfaced by more than one selector,
/// e.g. an `<a>` link and an `<iframe>` embedding the same video) are
/// deduplicated by URL so a single resource is not recorded twice.
/// ADAP-19: YouTube watch and embed URLs for the same video id are canonicalized
/// to the watch form before dedup, so `<a href="youtube.com/watch?v=X">` and
/// `<iframe src="youtube.com/embed/X">` collapse into one resource.
pub fn detect_media(document: &Html, base_url: &Url, source_id: &str) -> Vec<MediaResource> {
    let mut results = Vec::new();
    let mut seen: std::collections::HashSet<Url> = std::collections::HashSet::new();
    let mut push_dedup = |media: MediaResource, results: &mut Vec<MediaResource>| {
        let canonical = canonical_media_url(&media.url);
        if seen.insert(canonical) {
            results.push(media);
        }
    };

    if let Some(selector) = cached_selector("a") {
        for element in document.select(selector) {
            if let Some(href) = element.attr("href")
                && let Ok(resolved) = base_url.join(href)
            {
                let title_attr = element.attr("title");
                if let Some(media) =
                    classify_link(&resolved, &element, title_attr, base_url, source_id)
                {
                    push_dedup(media, &mut results);
                }
            }
        }
    }

    if let Some(selector) = cached_selector("iframe") {
        for element in document.select(selector) {
            if let Some(src) = element.attr("src")
                && let Ok(resolved) = base_url.join(src)
                && classify_video_platform(&resolved).is_some()
            {
                push_dedup(make_video(&resolved, base_url, source_id), &mut results);
            }
        }
    }

    if let Some(selector) = cached_selector("video") {
        for element in document.select(selector) {
            if let Some(src) = element.attr("src")
                && let Ok(resolved) = base_url.join(src)
            {
                push_dedup(make_video(&resolved, base_url, source_id), &mut results);
            }
            for child in element.children().filter_map(scraper::ElementRef::wrap) {
                if child.value().name() == "source"
                    && let Some(src) = child.attr("src")
                    && let Ok(resolved) = base_url.join(src)
                {
                    push_dedup(make_video(&resolved, base_url, source_id), &mut results);
                }
            }
        }
    }

    if let Some(selector) = cached_selector("audio") {
        for element in document.select(selector) {
            if let Some(src) = element.attr("src")
                && let Ok(resolved) = base_url.join(src)
            {
                push_dedup(make_audio(&resolved, base_url, source_id), &mut results);
            }
            for child in element.children().filter_map(scraper::ElementRef::wrap) {
                if child.value().name() == "source"
                    && let Some(src) = child.attr("src")
                    && let Ok(resolved) = base_url.join(src)
                {
                    push_dedup(make_audio(&resolved, base_url, source_id), &mut results);
                }
            }
        }
    }

    results
}

fn classify_link(
    url: &Url,
    element: &scraper::ElementRef,
    title_attr: Option<&str>,
    base_url: &Url,
    source_id: &str,
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
            source: make_source_evidence(base_url, source_id),
        });
    }
    if is_pdf(url) {
        let link_text = element.text().collect::<String>();
        let mut context = link_text;
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
            source: make_source_evidence(base_url, source_id),
        });
    }
    if let Some(media_type) = classify_raw_media(url) {
        return Some(MediaResource {
            id: MediaId(deterministic_id(&[url.as_str()])),
            media_type,
            title: None,
            url: url.clone(),
            platform: None,
            public_access: PublicAccess::Unknown,
            published_at: None,
            source: make_source_evidence(base_url, source_id),
        });
    }
    None
}

fn make_video(url: &Url, base_url: &Url, source_id: &str) -> MediaResource {
    MediaResource {
        id: MediaId(deterministic_id(&[url.as_str()])),
        media_type: MediaType::Video,
        title: None,
        url: url.clone(),
        platform: classify_video_platform(url).map(|s| s.into()),
        public_access: PublicAccess::Unknown,
        published_at: None,
        source: make_source_evidence(base_url, source_id),
    }
}

fn make_audio(url: &Url, base_url: &Url, source_id: &str) -> MediaResource {
    MediaResource {
        id: MediaId(deterministic_id(&[url.as_str()])),
        media_type: MediaType::Audio,
        title: None,
        url: url.clone(),
        platform: None,
        public_access: PublicAccess::Unknown,
        published_at: None,
        source: make_source_evidence(base_url, source_id),
    }
}

fn classify_video_platform(url: &Url) -> Option<&'static str> {
    let host = url.host_str().unwrap_or("");
    if is_youtube_host(host) {
        let path = url.path();
        let is_youtube_path = host == "youtu.be"
            || path == "/watch"
            || path.starts_with("/watch/")
            || path.contains("/embed/")
            || path.contains("/shorts/");
        if is_youtube_path {
            return Some("youtube");
        }
    }
    let is_vimeo = host == "vimeo.com" || host == "www.vimeo.com" || host.ends_with(".vimeo.com");
    if is_vimeo {
        return Some("vimeo");
    }
    let is_bilibili =
        host == "bilibili.com" || host == "www.bilibili.com" || host.ends_with(".bilibili.com");
    if is_bilibili && url.path().contains("/video/") {
        return Some("bilibili");
    }
    None
}

fn is_pdf(url: &Url) -> bool {
    url.path()
        .rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

/// True if `url` uses HTTP or HTTPS — the only schemes safe to store as an
/// event URL and eligible for fetch enrichment. Rejects `javascript:`,
/// `file:`, `data:`, `blob:`, `mailto:` etc. that can appear in untrusted
/// feed/page input and would otherwise be persisted in the state DB and
/// emitted in the public JSON output.
pub(crate) fn is_http_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// ADAP-18: classify direct links to raw audio/video files by extension.
/// A `<a href="talk.mp4">` link is a recording just as much as a `<video>` tag.
fn classify_raw_media(url: &Url) -> Option<MediaType> {
    let ext = url.path().rsplit('.').next()?;
    if ext.eq_ignore_ascii_case("mp4")
        || ext.eq_ignore_ascii_case("webm")
        || ext.eq_ignore_ascii_case("mkv")
        || ext.eq_ignore_ascii_case("mov")
        || ext.eq_ignore_ascii_case("avi")
        || ext.eq_ignore_ascii_case("m4v")
    {
        return Some(MediaType::Video);
    }
    if ext.eq_ignore_ascii_case("mp3")
        || ext.eq_ignore_ascii_case("ogg")
        || ext.eq_ignore_ascii_case("opus")
        || ext.eq_ignore_ascii_case("wav")
        || ext.eq_ignore_ascii_case("m4a")
        || ext.eq_ignore_ascii_case("aac")
        || ext.eq_ignore_ascii_case("flac")
    {
        return Some(MediaType::Audio);
    }
    None
}

/// ADAP-19: canonicalize a YouTube URL to its watch form so the same video
/// surfaced as a watch link and an embed iframe dedupes to one resource.
/// Returns the original URL unchanged for non-YouTube URLs.
fn canonical_media_url(url: &Url) -> Url {
    if !is_youtube_host(url.host_str().unwrap_or("")) {
        return url.clone();
    }
    let Some(id) = extract_youtube_id(url) else {
        return url.clone();
    };
    match Url::parse(&format!("https://www.youtube.com/watch?v={id}")) {
        Ok(u) => u,
        Err(_) => url.clone(),
    }
}

/// H2: exact-host YouTube detection. `host.ends_with("youtube.com")` would
/// also match attacker-controlled siblings like `notyoutube.com`. Match the
/// precise set of YouTube hosts instead.
fn is_youtube_host(host: &str) -> bool {
    host == "youtube.com"
        || host == "www.youtube.com"
        || host == "m.youtube.com"
        || host == "music.youtube.com"
        || host == "youtube-nocookie.com"
        || host.ends_with(".youtube-nocookie.com")
        || host.ends_with(".youtube.com")
        || host == "youtu.be"
}

/// Extract the 11-character video id from any YouTube URL form
/// (watch?v=, youtu.be/, /embed/, /shorts/). Returns None for malformed URLs
/// or ids that are not exactly 11 characters (YouTube's canonical id length).
fn extract_youtube_id(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    if !is_youtube_host(host) {
        return None;
    }
    if host == "youtu.be" {
        let segment = url.path().trim_start_matches('/');
        return valid_youtube_id(segment).map(|s| s.to_string());
    }
    let path = url.path();
    if path.contains("/embed/") || path.contains("/shorts/") {
        // H2-1: skip empty trailing segment so /embed/ABC/ still resolves.
        let segment = path.rsplit('/').find(|s| !s.is_empty())?;
        return valid_youtube_id(segment).map(|s| s.to_string());
    }
    url.query_pairs()
        .find(|(k, _)| k == "v")
        .and_then(|(_, v)| valid_youtube_id(&v).map(|s| s.to_string()))
}

/// H2: YouTube video ids are exactly 11 characters from `[A-Za-z0-9_-]`. The
/// previous `.get(0..11)` silently truncated longer ids (which can appear in
/// malformed URLs) and accepted shorter ones, producing wrong canonical URLs
/// that would never dedupe correctly.
fn valid_youtube_id(s: &str) -> Option<&str> {
    if s.len() == 11
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(s)
    } else {
        None
    }
}

fn make_source_evidence(base_url: &Url, source_id: &str) -> SourceEvidence {
    SourceEvidence {
        source_id: source_id.to_string(),
        source_url: base_url.clone(),
        evidence: None,
        captured_at: None,
        native_id: None,
    }
}

// ===========================================================================
// classify_access
// ===========================================================================

/// Stream `s` into `buf` lowercased with internal whitespace collapsed to
/// single spaces. `prev_space` tracks whether the last emitted char was a
/// space (start `true` to trim leading whitespace). Mirrors `normalize_text`
/// semantics but appends into an existing buffer so the access classifier can
/// build one lowercase-collapsed buffer in a single pass instead of allocating
/// a full-body String and re-normalizing it.
fn push_lower_collapsed(s: &str, buf: &mut String, prev_space: &mut bool) {
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !*prev_space {
                buf.push(' ');
            }
            *prev_space = true;
        } else {
            for lc in ch.to_lowercase() {
                buf.push(lc);
            }
            *prev_space = false;
        }
    }
}

/// Conservatively classify the public access level from an HTML document's
/// visible text and meta tags. Uses word-boundary matching to avoid
/// false positives like "free" inside "freedom" or "sso" inside a longer word.
pub fn classify_access(document: &Html) -> PublicAccess {
    // Build a single lowercase, whitespace-collapsed buffer in one pass over
    // the document's text nodes and <meta content> values. The keyword
    // matching only needs a lowercase substring search, so this avoids the
    // intermediate full-body String plus the `normalize_text` re-allocation
    // (O(2× body size) → O(body size)).
    let mut text = String::new();
    let mut prev_space = true;
    for t in document.root_element().text() {
        push_lower_collapsed(t, &mut text, &mut prev_space);
    }
    if let Some(selector) = cached_selector("meta") {
        for element in document.select(selector) {
            if let Some(content) = element.attr("content") {
                push_lower_collapsed(content, &mut text, &mut prev_space);
            }
        }
    }
    // Trim trailing whitespace (mirrors `normalize_text`'s `trim_end`).
    while text.ends_with(' ') {
        text.pop();
    }

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
    // R9-M03: negation phrases that suppress the REGISTRATION match. Without
    // this guard, "no registration required" matches REGISTRATION's
    // "registration required" substring (multi-word phrases use plain
    // `str::contains`) and is classified as RegistrationRequired instead of
    // Open. The negation only suppresses REGISTRATION — PAYWALLED and LOGIN
    // still win when present, since "no registration required, subscription
    // needed" is still paywalled.
    const REGISTRATION_NEGATIONS: &[&str] = &[
        "no registration required",
        "no registration needed",
        "registration not required",
        "registration not necessary",
        "no sign up required",
        "no sign up needed",
        "sign up not required",
    ];

    let registration_negated = REGISTRATION_NEGATIONS
        .iter()
        .any(|m| contains_phrase(&text, m));

    if PAYWALLED.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::Paywalled;
    }
    if LOGIN.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::InstitutionLogin;
    }
    if !registration_negated && REGISTRATION.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::RegistrationRequired;
    }
    if OPEN.iter().any(|m| contains_phrase(&text, m)) {
        return PublicAccess::Open;
    }
    // A registration negation by itself explicitly signals open access even
    // when no other OPEN keyword is present (e.g. "registration not required").
    if registration_negated {
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

/// Build an [`Event`] from adapter-extracted fields, filling the fixed
/// scaffolding (id from `title+url`, empty `topics`, zero `score`, default
/// `score_components`, empty `rank_reasons`, `None` first/last_seen) that
/// every adapter sets identically. Pass the extracted `title`/`url`/`source`
/// (may differ from the stub when the detail page corrected them) and the
/// per-adapter fields. `topics` is always empty here; the scan pipeline
/// enriches topics later via `radar_core::enrich_event_topics`.
#[allow(clippy::too_many_arguments)] // Event has 17 fields; 5 are fixed scaffolding, 12 are per-adapter
pub(crate) fn build_event_from_stub(
    title: &str,
    url: &Url,
    source: &SourceEvidence,
    event_type: EventType,
    status: EventStatus,
    date: EventDate,
    location: Option<Location>,
    description: Option<String>,
    people: Vec<PersonHit>,
    talks: Vec<Talk>,
    media: Vec<MediaResource>,
    access: AccessInfo,
) -> Event {
    Event {
        id: event_id(title, url.as_str()),
        title: title.to_string(),
        url: Some(url.clone()),
        event_type,
        status,
        date,
        location,
        description,
        topics: Vec::new(),
        people,
        talks,
        media,
        access,
        sources: vec![source.clone()],
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
        let fields = extract_html_fields(&doc(html));
        assert_eq!(fields.title.as_deref(), Some("Real Title"));
        assert_eq!(fields.description.as_deref(), Some("A test description"));
        assert_eq!(fields.date_text.as_deref(), Some("2026-08-08"));
        assert_eq!(fields.location_text.as_deref(), Some("Berlin, Germany"));
    }

    #[test]
    fn html_fields_empty() {
        let fields = extract_html_fields(&doc(""));
        assert!(fields.title.is_none());
        assert!(fields.description.is_none());
        assert!(fields.date_text.is_none());
        assert!(fields.location_text.is_none());
    }

    #[test]
    fn html_fields_malformed_no_panic() {
        let html = "<<<>><html><head><body>broken";
        let fields = extract_html_fields(&doc(html));
        let _ = fields;
    }

    #[test]
    fn html_fields_title_fallbacks() {
        let html = "<html><head><title>Title Tag</title></head><body></body></html>";
        let fields = extract_html_fields(&doc(html));
        assert_eq!(fields.title.as_deref(), Some("Title Tag"));

        let html = r#"<html><head><meta property="og:title" content="OG Title"></head><body></body></html>"#;
        let fields = extract_html_fields(&doc(html));
        assert_eq!(fields.title.as_deref(), Some("OG Title"));
    }

    #[test]
    fn html_fields_description_fallback_to_p() {
        let html = r#"<html><body><p>short</p><p>This is a longer paragraph with more than twenty characters.</p></body></html>"#;
        let fields = extract_html_fields(&doc(html));
        assert_eq!(
            fields.description.as_deref(),
            Some("This is a longer paragraph with more than twenty characters.")
        );
    }

    #[test]
    fn html_fields_date_from_class() {
        let html = r#"<html><body><div class="event-date">August 8, 2026</div></body></html>"#;
        let fields = extract_html_fields(&doc(html));
        assert_eq!(fields.date_text.as_deref(), Some("August 8, 2026"));
    }

    // --- detect_media ---

    #[test]
    fn media_youtube_link() {
        let html = r#"<a href="https://www.youtube.com/watch?v=abc123">Watch</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    // R9-M05: detect_media must propagate source_id into every MediaResource's
    // SourceEvidence so media records carry provenance. Previously
    // make_source_evidence hardcoded an empty string.
    #[test]
    fn media_carries_source_id() {
        let html = r#"<a href="https://www.youtube.com/watch?v=abc123">Watch</a>"#;
        let media = detect_media(&doc(html), &base(), "my-source-id");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].source.source_id, "my-source-id");
    }

    #[test]
    fn media_pdf_carries_source_id() {
        let html = r#"<a href="https://example.com/slides.pdf">Download slides</a>"#;
        let media = detect_media(&doc(html), &base(), "pdf-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].source.source_id, "pdf-source");
    }

    #[test]
    fn media_iframe_carries_source_id() {
        let html = r#"<iframe src="https://www.youtube.com/embed/abc123"></iframe>"#;
        let media = detect_media(&doc(html), &base(), "iframe-src");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].source.source_id, "iframe-src");
    }

    #[test]
    fn media_video_tag_carries_source_id() {
        let html = r#"<video src="https://example.com/talk.mp4"></video>"#;
        let media = detect_media(&doc(html), &base(), "video-src");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].source.source_id, "video-src");
    }

    #[test]
    fn media_pdf_slides() {
        let html = r#"<a href="https://example.com/slides.pdf">Download slides</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Slides);
    }

    #[test]
    fn media_pdf_program() {
        let html = r#"<a href="https://example.com/program.pdf">Program</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::ProgramPdf);
    }

    #[test]
    fn media_pdf_abstract() {
        let html = r#"<a href="https://example.com/abstract.pdf">Abstract</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::AbstractPdf);
    }

    #[test]
    fn media_pdf_other() {
        let html = r#"<a href="https://example.com/paper.pdf">Paper</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Other);
    }

    #[test]
    fn media_empty() {
        let media = detect_media(&doc(""), &base(), "test-source");
        assert!(media.is_empty());
    }

    #[test]
    fn media_vimeo() {
        let html = r#"<a href="https://vimeo.com/12345">Video</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("vimeo"));
    }

    #[test]
    fn media_bilibili() {
        let html = r#"<a href="https://www.bilibili.com/video/BV1234">Video</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("bilibili"));
    }

    #[test]
    fn media_iframe_video_platform() {
        let html = r#"<iframe src="https://www.youtube.com/embed/abc123"></iframe>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    #[test]
    fn media_iframe_non_video_ignored() {
        let html = r#"<iframe src="https://calendar.google.com/embed"></iframe>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert!(media.is_empty());
    }

    #[test]
    fn media_video_element_source_child() {
        let html = r#"<video><source src="https://example.com/talk.mp4"></video>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].url.as_str(), "https://example.com/talk.mp4");
    }

    #[test]
    fn media_audio_element() {
        let html = r#"<audio src="https://example.com/talk.mp3"></audio>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Audio);
    }

    #[test]
    fn media_audio_element_source_child() {
        let html = r#"<audio><source src="https://example.com/talk.opus"></audio>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Audio);
        assert_eq!(media[0].url.as_str(), "https://example.com/talk.opus");
    }

    #[test]
    fn media_relative_url() {
        let html = r#"<a href="/talks/video.pdf">slides</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].url.as_str(), "https://example.com/talks/video.pdf");
        assert_eq!(media[0].media_type, MediaType::Slides);
    }

    #[test]
    fn media_youtu_be() {
        let html = r#"<a href="https://youtu.be/abc123">Short link</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    // ADAP-17: youtube-nocookie.com embed iframes are a common privacy-
    // preserving embed form and must be classified as YouTube videos.
    #[test]
    fn media_youtube_nocookie_embed() {
        let html = r#"<iframe src="https://www.youtube-nocookie.com/embed/abc123"></iframe>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    // ADAP-17: youtube.com/shorts/ links are a growing video form.
    #[test]
    fn media_youtube_shorts_link() {
        let html = r#"<a href="https://www.youtube.com/shorts/abc12345678">Short</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    // ADAP-18: direct links to raw media files must be classified, not just
    // <video>/<audio> elements. A page linking <a href="talk.mp4"> is a
    // recording just as much as <video src="talk.mp4">.
    #[test]
    fn media_raw_video_link() {
        let html = r#"<a href="https://example.com/lecture.mp4">Recording</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Video);
    }

    #[test]
    fn media_raw_audio_link() {
        let html = r#"<a href="https://example.com/talk.mp3">Audio</a>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_type, MediaType::Audio);
    }

    // ADAP-19: the same YouTube video surfaced as both a watch link and an
    // embed iframe must dedupe to one resource. The URLs differ
    // (watch?v=X vs /embed/X) so naive URL dedup would record it twice.
    #[test]
    fn media_youtube_watch_and_embed_dedup() {
        let html = r#"<a href="https://www.youtube.com/watch?v=abc12345678">Watch</a>
        <iframe src="https://www.youtube.com/embed/abc12345678"></iframe>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(
            media.len(),
            1,
            "watch link and embed of same video must dedupe"
        );
        assert_eq!(media[0].platform.as_deref(), Some("youtube"));
    }

    #[test]
    fn media_youtu_be_and_embed_dedup() {
        let html = r#"<a href="https://youtu.be/abc12345678">Short</a>
        <iframe src="https://www.youtube-nocookie.com/embed/abc12345678"></iframe>"#;
        let media = detect_media(&doc(html), &base(), "test-source");
        assert_eq!(
            media.len(),
            1,
            "youtu.be and nocookie embed of same video must dedupe"
        );
    }

    #[test]
    fn media_malformed_no_panic() {
        let html = "<<<>><a href=>broken</a>";
        let media = detect_media(&doc(html), &base(), "test-source");
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

    // R9-M03: "no registration required" must NOT be classified as
    // RegistrationRequired (it contains the substring "registration required"
    // but the negation guard suppresses the REGISTRATION match).
    #[test]
    fn access_no_registration_required_is_open() {
        let html = "<html><body>No registration required. All welcome.</body></html>";
        assert_eq!(classify_access(&doc(html)), PublicAccess::Open);
    }

    #[test]
    fn access_registration_not_required_is_open() {
        let html = "<html><body>Registration not required for this event.</body></html>";
        assert_eq!(classify_access(&doc(html)), PublicAccess::Open);
    }

    #[test]
    fn access_no_registration_but_still_paywalled() {
        let html =
            "<html><body>No registration required, but subscription needed to view.</body></html>";
        assert_eq!(classify_access(&doc(html)), PublicAccess::Paywalled);
    }

    #[test]
    fn access_no_registration_but_still_login() {
        let html =
            "<html><body>No registration required, but SSO login required to access.</body></html>";
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
