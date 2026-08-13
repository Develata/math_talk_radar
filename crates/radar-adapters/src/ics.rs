//! iCalendar (ICS) feed adapter (§P-5 structured-source priority #3).
//!
//! Parses VEVENT entries from an ICS feed into [`EventStub`]s. A nesting-depth
//! guard (§67) rejects feeds with nesting deeper than 8 levels before parsing
//! to prevent a stack-overflow DoS in `icalendar`'s nom recursive-descent
//! parser. The guard tracks actual BEGIN/END nesting depth (not flat component
//! count) so legitimate calendars with many flat VEVENTs are not rejected.
use radar_core::{
    AccessInfo, AdapterError, Event, EventCandidate, EventDate, EventStatus, EventStub, FetchPlan,
    FetchedDocument, Location, OnlineAvailability, PublicAccess, ScoreComponents, SourceAdapter,
    SourceEvidence, SourceSpec, event_id,
};
use url::Url;

use crate::helpers;

/// Maximum nesting depth before a feed is rejected as a potential DoS (§67).
/// Legitimate ICS nesting is at most 3 (VCALENDAR > VEVENT > VALARM or
/// VCALENDAR > VTIMEZONE > STANDARD). 8 gives ample headroom while staying far
/// below stack-overflow territory for `icalendar`'s nom parser.
const MAX_NESTING_DEPTH: usize = 8;

#[derive(Debug, Default)]
pub struct IcsAdapter;

impl SourceAdapter for IcsAdapter {
    fn discover(
        &self,
        document: &FetchedDocument,
        source: &SourceSpec,
    ) -> Result<Vec<EventStub>, AdapterError> {
        let depth = max_nesting_depth(&document.body);
        if depth > MAX_NESTING_DEPTH {
            return Err(AdapterError::Parse {
                source_id: source.id.clone(),
                message: format!("ICS nesting depth {depth} > {MAX_NESTING_DEPTH}, possible DoS"),
            });
        }

        let text = crate::helpers::doc_body(&document.body);
        let unfolded = icalendar::parser::unfold(&text);
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
            reason: "ics_detail".into(),
        }]
    }

    fn enrich(
        &self,
        event: EventStub,
        documents: &[FetchedDocument],
        _source: &SourceSpec,
    ) -> Result<EventCandidate, AdapterError> {
        let event_type = helpers::detect_event_type(&event.title);
        let date = event
            .date_hint
            .clone()
            .unwrap_or_else(|| EventDate::unknown(String::new()));

        let mut description = None;
        let mut location = None;
        if let Some(doc) = documents.first()
            && doc.body.contains(&b'<')
        {
            let body = helpers::doc_body(&doc.body);
            let document = scraper::Html::parse_document(&body);
            let fields = helpers::extract_html_fields(&document);
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

        let id = event_id(&event.title, event.url.as_str());
        let full_event = Event {
            id,
            title: event.title.clone(),
            url: Some(event.url.clone()),
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

/// Track the maximum nesting depth of BEGIN:/END: blocks (case-insensitive).
/// Used by the §67 depth guard: flat calendars with many VEVENTs have depth 2
/// (VCALENDAR > VEVENT), while a malicious deeply nested payload has depth
/// proportional to the nesting.
fn max_nesting_depth(body: &[u8]) -> usize {
    let text = crate::helpers::doc_body(body);
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;
    for line in text.lines() {
        if line
            .get(..6)
            .is_some_and(|p| p.eq_ignore_ascii_case("BEGIN:"))
        {
            depth += 1;
            if depth > max_depth {
                max_depth = depth;
            }
        } else if line
            .get(..4)
            .is_some_and(|p| p.eq_ignore_ascii_case("END:"))
        {
            depth = depth.saturating_sub(1);
        }
    }
    max_depth
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
        // 33 levels of nested BEGIN: → depth 33 > 8 → Err(Parse), no crash.
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
            "depth guard should reject nesting depth >8"
        );
    }

    #[test]
    fn discover_depth_guard_allows_many_flat_events() {
        // 100 flat VEVENTs → nesting depth 2 (VCALENDAR > VEVENT) → allowed.
        let mut body = String::from("BEGIN:VCALENDAR\nVERSION:2.0\n");
        for i in 0..100 {
            body.push_str(&format!(
                "BEGIN:VEVENT\nUID:e{i}\nSUMMARY:Event {i}\nURL:https://example.com/e{i}\nDTSTART:20260808\nEND:VEVENT\n"
            ));
        }
        body.push_str("END:VCALENDAR\n");
        let doc = make_doc(body.as_bytes());
        let source = make_source();
        let stubs = IcsAdapter
            .discover(&doc, &source)
            .expect("flat calendar with depth 2 should be allowed");
        assert_eq!(stubs.len(), 100);
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
                native_id: None,
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
                native_id: None,
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
                native_id: None,
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
    fn max_nesting_depth_cases() {
        assert_eq!(max_nesting_depth(b"BEGIN:VCALENDAR\nEND:VCALENDAR\n"), 1);
        assert_eq!(
            max_nesting_depth(b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nEND:VEVENT\nEND:VCALENDAR\n"),
            2
        );
        assert_eq!(
            max_nesting_depth(b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nBEGIN:VALARM\nEND:VALARM\nEND:VEVENT\nEND:VCALENDAR\n"),
            3
        );
        assert_eq!(max_nesting_depth(b"begin:vcalendar\n"), 1);
        assert_eq!(max_nesting_depth(b"Begin:VCALENDAR\n"), 1);
        assert_eq!(max_nesting_depth(b"X-FOO:bar\n"), 0);
        assert_eq!(max_nesting_depth(b""), 0);
        assert_eq!(max_nesting_depth(b" BEGIN:foo\n"), 0);
        // Many flat VEVENTs → depth 2, not 33.
        let flat =
            b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nEND:VEVENT\nBEGIN:VEVENT\nEND:VEVENT\nEND:VCALENDAR\n";
        assert_eq!(max_nesting_depth(flat), 2);
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
