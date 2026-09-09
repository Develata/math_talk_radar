//! Media link recognition, platform classification and URL identity.
use super::cached_selector;
use radar_core::{
    MediaId, MediaResource, MediaType, PublicAccess, SourceEvidence, deterministic_id,
};
use scraper::Html;
use url::Url;

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
    let s = url.as_str();
    if s.contains("youtube.com/watch")
        || s.contains("youtu.be/")
        || s.contains("youtube.com/embed")
        || s.contains("youtube-nocookie.com/embed")
        || s.contains("youtube.com/shorts")
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

/// ADAP-18: classify direct links to raw audio/video files by extension.
/// A `<a href="talk.mp4">` link is a recording just as much as a `<video>` tag.
fn classify_raw_media(url: &Url) -> Option<MediaType> {
    let path = url.path().to_lowercase();
    let ext = path.rsplit('.').next()?;
    match ext {
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" => Some(MediaType::Video),
        "mp3" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "flac" => Some(MediaType::Audio),
        _ => None,
    }
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
