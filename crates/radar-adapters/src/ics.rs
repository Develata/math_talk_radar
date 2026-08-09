//! iCalendar (ICS) feed adapter (§P-5 structured-source priority #3).
//!
//! Parses VEVENT entries from an ICS feed into [`EventStub`]s. A nesting-depth
//! guard (§67) rejects feeds with more than 32 `BEGIN:` lines before parsing to
//! prevent a stack-overflow DoS in `icalendar`'s nom recursive-descent parser.
use radar_core::{
    AccessInfo, AdapterError, DatePrecision, Event, EventCandidate, EventDate, EventId,
    EventStatus, EventStub, FetchPlan, FetchedDocument, Location, OnlineAvailability, PublicAccess,
    ScoreComponents, SourceAdapter, SourceEvidence, SourceSpec, deterministic_id,
};
use url::Url;

use crate::helpers;

/// Maximum `BEGIN:` lines before a feed is rejected as a potential DoS (§67).
const MAX_BEGIN_LINES: usize = 32;

#[derive(Debug, Default)]
pub struct IcsAdapter;

impl SourceAdapter for IcsAdapter {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        // §67 nesting-depth guard: count BEGIN: lines (case-insensitive,
        // line-start) before parsing to prevent a stack-overflow DoS.
        let begin_count = count_begin_lines(&document.body);
        if begin_count > MAX_BEGIN_LINES {
            return Err(AdapterError::Parse {
                source_id: source.id.clone(),
                message: "ICS nesting depth >32, possible DoS".into(),
            });
        }

        let text = std::str::from_utf8(&document.body).unwrap_or("");
        let unfolded = icalendar::parser::unfold(text);
        let calendar =
            icalendar::parser::read_calendar(&unfolded).map_err(|e| AdapterError::Parse {
                source_id: source.id.clone(),
                message: format!("ICS parse error: {e}"),
            })?;

        let mut stubs = Vec::new();
        for component in &calendar.components {
            if component.name.as_str() != "VEVENT" {
                continue;
            }
            let title = component
                .find_prop("SUMMARY")
                .map(|p| p.val.as_str().to_string());
            let url_str = component
                .find_prop("URL")
                .map(|p| p.val.as_str().to_string());
            let dtstart = component
                .find_prop("DTSTART")
                .map(|p| p.val.as_str().to_string());

            let Some(title) = title else { continue };
            let Some(url_str) = url_str else { continue };
            if title.trim().is_empty() {
                continue;
            }
            let Ok(url) = Url::parse(&url_str) else {
                continue;
            };

            let date_hint = dtstart.as_deref().and_then(parse_ics_dtstart);

            stubs.push(EventStub {
                title,
                url,
                date_hint,
                source: SourceEvidence {
                    source_id: source.id.clone(),
                    source_url: document.url.clone(),
                    evidence: Some("ics".into()),
                    captured_at: Some(document.fetched_at),
                },
            });
        }

        Ok(stubs)
    }

    fn plan_enrichment(&self, event: &EventStub, _source: &SourceSpec) -> Vec<FetchPlan> {
        vec![FetchPlan {
            url: event.url.clone(),
            depth: 1,
            reason: "ics_detail".into(),
        }]
    }

    fn enrich(
        &self,
        event: EventStub,
        documents: &[FetchedDocument],
        source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        let event_type = helpers::detect_event_type(&event.title);
        let date = event.date_hint.clone().unwrap_or_else(|| EventDate {
            start: None,
            end: None,
            timezone: None,
            original_text: String::new(),
            precision: DatePrecision::Unknown,
        });

        let mut description = None;
        let mut location = None;
        if let Some(doc) = documents.first()
            && let Ok(body) = std::str::from_utf8(&doc.body)
            && body.contains('<')
        {
            let fields = helpers::extract_html_fields(body, &doc.url);
            description = fields.description;
            if let Some(loc_text) = fields.location_text {
                location = Some(Location {
                    name: loc_text,
                    city: None,
                    country: None,
                    venue: None,
                });
            }
        }

        let id = EventId(deterministic_id(&[&event.title, &source.id]));
        let full_event = Event {
            id,
            title: event.title.clone(),
            event_type,
            status: EventStatus::Announced,
            date,
            location,
            description,
            topics: Vec::new(),
            people: Vec::new(),
            talks: Vec::new(),
            media: Vec::new(),
            access: AccessInfo {
                access: PublicAccess::Unknown,
                online: OnlineAvailability::Unknown,
            },
            sources: vec![event.source.clone()],
            score: 0.0,
            score_components: ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        };

        Ok(EventCandidate {
            event: full_event,
            stub: event,
        })
    }
}

/// Count lines starting with `BEGIN:` (case-insensitive) in the raw body.
/// Used by the §67 nesting-depth guard before parsing.
fn count_begin_lines(body: &[u8]) -> usize {
    let text = std::str::from_utf8(body).unwrap_or("");
    text.lines()
        .filter(|line| {
            line.get(..6)
                .is_some_and(|p| p.eq_ignore_ascii_case("BEGIN:"))
        })
        .count()
}

/// Parse an ICS DTSTART value into an [`EventDate`]. Only the date portion is
/// extracted; time and timezone are dropped (the result feeds into
/// [`radar_core::date::parse_date`] which is date-only). Returns `None` for
/// unparseable values.
fn parse_ics_dtstart(value: &str) -> Option<EventDate> {
    let bytes = value.as_bytes();
    if bytes.len() < 8 {
        return None;
    }
    let date_part = std::str::from_utf8(&bytes[..8]).ok()?;
    if !date_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let iso = format!(
        "{}-{}-{}",
        &date_part[..4],
        &date_part[4..6],
        &date_part[6..8]
    );
    let mut ed = radar_core::date::parse_date(&iso).ok()?;
    ed.original_text = value.to_string();
    Some(ed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radar_core::{AdapterKind, EventType, SourceKind, SourceTier};

    fn make_doc(body: &[u8]) -> FetchedDocument {
        FetchedDocument {
            url: Url::parse("https://example.com/cal.ics").unwrap(),
            final_url: Url::parse("https://example.com/cal.ics").unwrap(),
            status: 200,
            content_type: Some("text/calendar".into()),
            body: body.to_vec(),
            fetched_at: Default::default(),
        }
    }

    fn make_source() -> SourceSpec {
        SourceSpec {
            id: "test-ics".to_string(),
            name: "Test ICS".to_string(),
            tier: SourceTier::Unknown,
            kind: SourceKind::IcsFeed,
            adapter: AdapterKind::Ics,
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

    const THREE_EVENT_ICS: &str = "\
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//math_talk_radar//test//EN
BEGIN:VEVENT
UID:event-1
SUMMARY:Conference on Algebra
URL:https://example.com/event1
DTSTART:20260808
END:VEVENT
BEGIN:VEVENT
UID:event-2
SUMMARY:Workshop on Graph Theory
URL:https://example.com/event2
DTSTART:20260809T120000Z
END:VEVENT
BEGIN:VEVENT
UID:event-3
SUMMARY:Summer School on Topology
URL:https://example.com/event3
DTSTART;VALUE=DATE:20260810
END:VEVENT
END:VCALENDAR
";

    #[test]
    fn discover_three_events() {
        let doc = make_doc(THREE_EVENT_ICS.as_bytes());
        let source = make_source();
        let stubs = IcsAdapter
            .discover(&doc, &source)
            .expect("valid ICS should parse");
        assert_eq!(stubs.len(), 3);
        assert_eq!(stubs[0].title, "Conference on Algebra");
        assert_eq!(stubs[0].url.as_str(), "https://example.com/event1");
        assert!(stubs[0].date_hint.is_some());
        assert_eq!(stubs[1].title, "Workshop on Graph Theory");
        assert_eq!(stubs[1].url.as_str(), "https://example.com/event2");
        assert_eq!(stubs[2].title, "Summer School on Topology");
        assert_eq!(stubs[2].url.as_str(), "https://example.com/event3");
    }

    #[test]
    fn discover_skips_events_without_url() {
        let ics = "\
BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:no-url
SUMMARY:No URL Event
DTSTART:20260808
END:VEVENT
BEGIN:VEVENT
UID:with-url
SUMMARY:With URL Event
URL:https://example.com/e
DTSTART:20260809
END:VEVENT
END:VCALENDAR
";
        let doc = make_doc(ics.as_bytes());
        let source = make_source();
        let stubs = IcsAdapter
            .discover(&doc, &source)
            .expect("valid ICS should parse");
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].title, "With URL Event");
    }

    #[test]
    fn discover_event_type_via_enrich() {
        let doc = make_doc(THREE_EVENT_ICS.as_bytes());
        let source = make_source();
        let stubs = IcsAdapter.discover(&doc, &source).expect("parse ok");
        let candidate = IcsAdapter
            .enrich(stubs.into_iter().next().unwrap(), &[], &source)
            .expect("enrich ok");
        assert_eq!(candidate.event.event_type, EventType::Conference);
    }

    #[test]
    fn discover_date_hint_parsed() {
        let doc = make_doc(THREE_EVENT_ICS.as_bytes());
        let source = make_source();
        let stubs = IcsAdapter.discover(&doc, &source).expect("parse ok");
        let dh = stubs[0]
            .date_hint
            .as_ref()
            .expect("date hint should be present");
        assert!(dh.start.is_some());
        assert_eq!(dh.original_text, "20260808");
    }

    #[test]
    fn discover_depth_guard_rejects_deep_nesting() {
        // 33 BEGIN: lines → exceeds the 32 limit → Err(Parse), no crash.
        let mut body = String::new();
        for _ in 0..33 {
            body.push_str("BEGIN:VCALENDAR\n");
        }
        for _ in 0..33 {
            body.push_str("END:VCALENDAR\n");
        }
        let doc = make_doc(body.as_bytes());
        let source = make_source();
        let result = IcsAdapter.discover(&doc, &source);
        assert!(
            matches!(result, Err(AdapterError::Parse { .. })),
            "depth guard should reject >32 BEGIN: lines"
        );
    }

    #[test]
    fn discover_depth_guard_allows_boundary() {
        // Exactly 32 BEGIN: lines (1 VCALENDAR + 31 VEVENTs) → allowed.
        let mut body = String::from("BEGIN:VCALENDAR\nVERSION:2.0\n");
        for i in 0..31 {
            body.push_str(&format!(
                "BEGIN:VEVENT\nUID:e{i}\nSUMMARY:Event {i}\nURL:https://example.com/e{i}\nDTSTART:20260808\nEND:VEVENT\n"
            ));
        }
        body.push_str("END:VCALENDAR\n");
        let doc = make_doc(body.as_bytes());
        let source = make_source();
        let stubs = IcsAdapter
            .discover(&doc, &source)
            .expect("32 BEGIN: lines should be allowed");
        assert_eq!(stubs.len(), 31);
    }

    #[test]
    fn discover_malformed_truncated() {
        let body = b"BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nSUMMARY:Truncated\n";
        let doc = make_doc(body);
        let source = make_source();
        let result = IcsAdapter.discover(&doc, &source);
        assert!(
            matches!(result, Err(AdapterError::Parse { .. })),
            "truncated ICS should return Err(Parse)"
        );
    }

    #[test]
    fn discover_empty_body() {
        let doc = make_doc(b"");
        let source = make_source();
        let result = IcsAdapter.discover(&doc, &source);
        assert!(
            matches!(result, Err(AdapterError::Parse { .. })),
            "empty body should return Err(Parse)"
        );
    }

    #[test]
    fn discover_no_panic_on_garbage() {
        let doc = make_doc(b"\xff\xfe\x00garbage not ics at all");
        let source = make_source();
        let _ = IcsAdapter.discover(&doc, &source);
    }

    #[test]
    fn plan_enrichment_returns_fetch() {
        let stub = EventStub {
            title: "Test Event".into(),
            url: Url::parse("https://example.com/e1").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-ics".into(),
                source_url: Url::parse("https://example.com/cal.ics").unwrap(),
                evidence: None,
                captured_at: None,
            },
        };
        let source = make_source();
        let plans = IcsAdapter.plan_enrichment(&stub, &source);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].url.as_str(), "https://example.com/e1");
        assert_eq!(plans[0].depth, 1);
        assert_eq!(plans[0].reason, "ics_detail");
    }

    #[test]
    fn enrich_minimal_event() {
        let stub = EventStub {
            title: "Conference on Algebra".into(),
            url: Url::parse("https://example.com/event1").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-ics".into(),
                source_url: Url::parse("https://example.com/cal.ics").unwrap(),
                evidence: Some("ics".into()),
                captured_at: None,
            },
        };
        let source = make_source();
        let candidate = IcsAdapter
            .enrich(stub, &[], &source)
            .expect("enrich should succeed");
        assert_eq!(candidate.event.title, "Conference on Algebra");
        assert_eq!(candidate.event.event_type, EventType::Conference);
        assert_eq!(candidate.event.status, EventStatus::Announced);
        assert_eq!(candidate.event.sources.len(), 1);
        assert!(candidate.event.location.is_none());
        assert!(candidate.event.description.is_none());
    }

    #[test]
    fn enrich_with_html_fields() {
        let stub = EventStub {
            title: "Workshop on Graph Theory".into(),
            url: Url::parse("https://example.com/event2").unwrap(),
            date_hint: None,
            source: SourceEvidence {
                source_id: "test-ics".into(),
                source_url: Url::parse("https://example.com/cal.ics").unwrap(),
                evidence: None,
                captured_at: None,
            },
        };
        let html_doc = FetchedDocument {
            url: Url::parse("https://example.com/event2").unwrap(),
            final_url: Url::parse("https://example.com/event2").unwrap(),
            status: 200,
            content_type: Some("text/html".into()),
            body: b"<html><head><meta name=\"description\" content=\"A great workshop\"></head><body><p class=\"location\">Berlin, Germany</p></body></html>".to_vec(),
            fetched_at: Default::default(),
        };
        let source = make_source();
        let candidate = IcsAdapter
            .enrich(stub, &[html_doc], &source)
            .expect("enrich should succeed");
        assert_eq!(
            candidate.event.description.as_deref(),
            Some("A great workshop")
        );
        assert_eq!(
            candidate.event.location.as_ref().map(|l| l.name.as_str()),
            Some("Berlin, Germany")
        );
    }

    #[test]
    fn count_begin_lines_cases() {
        assert_eq!(count_begin_lines(b"BEGIN:VCALENDAR\n"), 1);
        assert_eq!(
            count_begin_lines(b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nEND:VEVENT\nEND:VCALENDAR\n"),
            2
        );
        assert_eq!(count_begin_lines(b"begin:vcalendar\n"), 1);
        assert_eq!(count_begin_lines(b"Begin:VCALENDAR\n"), 1);
        assert_eq!(count_begin_lines(b"X-FOO:bar\n"), 0);
        assert_eq!(count_begin_lines(b""), 0);
        // folded continuation line (starts with space) should not count
        assert_eq!(count_begin_lines(b" BEGIN:foo\n"), 0);
    }

    #[test]
    fn parse_ics_dtstart_variants() {
        assert!(parse_ics_dtstart("20260808").is_some());
        assert!(parse_ics_dtstart("20260808T120000Z").is_some());
        assert!(parse_ics_dtstart("20260808T120000").is_some());
        assert!(parse_ics_dtstart("short").is_none());
        assert!(parse_ics_dtstart("XXXXXXXX").is_none());
    }
}
