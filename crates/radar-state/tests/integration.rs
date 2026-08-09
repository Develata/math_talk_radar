//! Integration tests for the state repository and change detection (§22, §23).
//!
//! Covers STATE-001..004 from `docs/acceptance-cases/06_state_changes.md`.
//! Uses `tempfile::TempDir` so each test gets an isolated redb database file.

use chrono::{DateTime, NaiveDate, Utc};
use radar_core::{
    AccessInfo, DatePrecision, DateTimeOrDate, Event, EventDate, EventId, EventStatus, EventType,
    Location, MediaId, MediaResource, MediaType, OnlineAvailability, PublicAccess, ScoreComponents,
    SourceEvidence,
};
use radar_state::{ChangeKind, Repository, detect_changes};
use url::Url;

fn t0() -> DateTime<Utc> {
    DateTime::<Utc>::from_naive_utc_and_offset(
        NaiveDate::from_ymd_opt(2026, 8, 9)
            .expect("valid date")
            .and_hms_opt(12, 0, 0)
            .expect("valid time"),
        Utc,
    )
}

fn t1() -> DateTime<Utc> {
    t0() + chrono::Duration::hours(24)
}

fn src() -> SourceEvidence {
    SourceEvidence {
        source_id: "s1".into(),
        source_url: Url::parse("https://example.com/feed").expect("valid url"),
        evidence: None,
        captured_at: None,
        native_id: None,
    }
}

fn base_event(id: &str, media: Vec<MediaResource>) -> Event {
    Event {
        id: EventId(id.into()),
        title: "Algebraic Geometry Conference".into(),
        url: Some(Url::parse("https://example.com/e1").expect("valid url")),
        event_type: EventType::Conference,
        status: EventStatus::Unknown,
        date: EventDate {
            start: Some(DateTimeOrDate::Date(
                NaiveDate::from_ymd_opt(2026, 8, 9).expect("valid date"),
            )),
            end: None,
            timezone: None,
            original_text: String::new(),
            precision: DatePrecision::Day,
        },
        location: Some(Location {
            name: "MIT".into(),
            city: None,
            country: None,
            venue: None,
        }),
        description: None,
        topics: Vec::new(),
        people: Vec::new(),
        talks: Vec::new(),
        media,
        access: AccessInfo {
            access: PublicAccess::Unknown,
            online: OnlineAvailability::Unknown,
        },
        sources: vec![src()],
        score: 0.0,
        score_components: ScoreComponents::default(),
        rank_reasons: Vec::new(),
        first_seen_at: None,
        last_seen_at: None,
    }
}

fn video(url: &str) -> MediaResource {
    MediaResource {
        id: MediaId("m1".into()),
        media_type: MediaType::Video,
        title: None,
        url: Url::parse(url).expect("valid url"),
        platform: None,
        public_access: PublicAccess::Open,
        published_at: None,
        source: src(),
    }
}

/// STATE-001: storing an event persists `first_seen`, which survives a close
/// and reopen of the repository.
#[test]
fn state_001_first_seen_persisted() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    let now = t0();
    let event = base_event("e1", vec![]);

    {
        let repo = Repository::open(&db_path).expect("open repo");
        let stored = repo.store_event(&event, now).expect("store event");
        assert_eq!(stored.first_seen_at, Some(now));
        assert_eq!(stored.last_seen_at, Some(now));
    }

    let repo = Repository::open(&db_path).expect("reopen repo");
    let retrieved = repo
        .get_event(&event.id)
        .expect("get event")
        .expect("event present after reopen");
    assert_eq!(retrieved.first_seen_at, Some(now));
    assert_eq!(retrieved.last_seen_at, Some(now));
}

/// STATE-002: re-storing an unchanged event emits no change records.
#[test]
fn state_002_second_scan_unchanged() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    let now = t0();
    let event = base_event("e1", vec![]);

    let repo = Repository::open(&db_path).expect("open repo");
    repo.store_event(&event, now).expect("first store");

    let previous = repo.list_events().expect("list events");
    let records = detect_changes(&previous, std::slice::from_ref(&event), now);
    assert!(
        records.is_empty(),
        "unchanged event should emit no change records, got {records:?}"
    );
}

/// STATE-003: canonical baseline — event first seen with `media = []`, then
/// re-seen with a new video, emits `MediaAdded`.
#[test]
fn state_003_media_added() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    let now = t0();
    let later = t1();

    let repo = Repository::open(&db_path).expect("open repo");
    let first = base_event("e1", vec![]);
    repo.store_event(&first, now).expect("first store");

    let previous = repo.list_events().expect("list events");

    let second = base_event("e1", vec![video("https://youtube.com/v/1")]);
    let records = detect_changes(&previous, std::slice::from_ref(&second), later);
    assert_eq!(records.len(), 1, "expected exactly one change record");
    assert_eq!(records[0].kind, ChangeKind::MediaAdded);
    assert_eq!(records[0].event_id, second.id);

    repo.store_event(&second, later).expect("second store");
    let after = repo
        .get_event(&second.id)
        .expect("get event")
        .expect("event present");
    assert_eq!(
        after.first_seen_at,
        Some(now),
        "first_seen must be preserved"
    );
    assert_eq!(after.last_seen_at, Some(later));
}

/// STATE-004: a read-only repository rejects writes without touching the
/// database file.
#[test]
fn state_004_no_state_no_write() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    let now = t0();
    let event = base_event("e1", vec![]);

    let repo = Repository::open(&db_path).expect("open repo");
    repo.store_event(&event, now).expect("seed event");
    drop(repo);

    let file_size_before = std::fs::metadata(&db_path).expect("db file exists").len();

    let ro = Repository::open_read_only(&db_path).expect("open read-only");
    assert!(ro.is_read_only());

    let result = ro.store_event(&event, now);
    assert!(
        result.is_err(),
        "write on a read-only repository must fail, got {result:?}"
    );

    let file_size_after = std::fs::metadata(&db_path)
        .expect("db file still exists")
        .len();
    assert_eq!(
        file_size_before, file_size_after,
        "read-only repository must not modify the database file"
    );
}
