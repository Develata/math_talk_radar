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
use radar_state::{ChangeKind, Repository, StateError, detect_changes};
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

fn t2() -> DateTime<Utc> {
    t0() + chrono::Duration::hours(48)
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

fn event_with_title(id: &str, title: &str) -> Event {
    let mut e = base_event(id, vec![]);
    e.title = title.into();
    e
}

/// store_scan on an empty repository with no current events produces no
/// changes and stores nothing.
#[test]
fn store_scan_empty() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let (stored, changes) = repo.store_scan(&[], t0()).expect("store_scan");
    assert!(stored.is_empty());
    assert!(changes.is_empty());
    assert!(repo.list_events().expect("list").is_empty());
}

/// store_scan of a new event emits EventAdded and stamps first/last seen.
#[test]
fn store_scan_new_event_emits_added() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let now = t0();
    let event = base_event("e1", vec![]);
    let (stored, changes) = repo
        .store_scan(std::slice::from_ref(&event), now)
        .expect("store_scan");

    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].first_seen_at, Some(now));
    assert_eq!(stored[0].last_seen_at, Some(now));

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::EventAdded);
    assert_eq!(changes[0].event_id, event.id);
}

/// store_scan of an unchanged event emits no change records but updates
/// last_seen_at; first_seen_at is preserved across scans.
#[test]
fn store_scan_unchanged_preserves_first_seen() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let first = t0();
    let later = t1();
    let event = base_event("e1", vec![]);

    let (stored1, changes1) = repo
        .store_scan(std::slice::from_ref(&event), first)
        .expect("scan 1");
    assert_eq!(changes1.len(), 1);
    assert_eq!(stored1[0].first_seen_at, Some(first));

    let (stored2, changes2) = repo
        .store_scan(std::slice::from_ref(&event), later)
        .expect("scan 2");
    assert!(
        changes2.is_empty(),
        "unchanged re-scan emits nothing: {changes2:?}"
    );
    assert_eq!(
        stored2[0].first_seen_at,
        Some(first),
        "first_seen preserved"
    );
    assert_eq!(stored2[0].last_seen_at, Some(later), "last_seen updated");
}

/// store_scan detects MediaAdded (canonical baseline §23).
#[test]
fn store_scan_media_added() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let first = base_event("e1", vec![]);
    repo.store_scan(std::slice::from_ref(&first), t0())
        .expect("scan 1");

    let second = base_event("e1", vec![video("https://youtube.com/v/1")]);
    let (stored, changes) = repo
        .store_scan(std::slice::from_ref(&second), t1())
        .expect("scan 2");

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::MediaAdded);
    assert_eq!(stored[0].first_seen_at, Some(t0()), "first_seen preserved");
    assert_eq!(stored[0].last_seen_at, Some(t1()));
}

/// store_scan detects EventCancelled when an event disappears.
#[test]
fn store_scan_event_cancelled() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let event = base_event("e1", vec![]);
    repo.store_scan(std::slice::from_ref(&event), t0())
        .expect("scan 1");

    let (stored, changes) = repo.store_scan(&[], t1()).expect("scan 2");
    assert!(stored.is_empty());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::EventCancelled);
}

/// ST M-1: a cancelled event is pruned from the DB so a subsequent scan does
/// NOT re-emit EventCancelled. Without pruning, detect_changes would see the
/// stale event in prev on every scan and re-emit EventCancelled forever, and
/// the DB would grow unboundedly.
#[test]
fn store_scan_cancelled_event_is_pruned_and_not_re_emitted() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let event = base_event("e1", vec![]);
    repo.store_scan(std::slice::from_ref(&event), t0())
        .expect("scan 1: seed");

    let (_, changes2) = repo.store_scan(&[], t1()).expect("scan 2: cancel");
    assert_eq!(changes2.len(), 1);
    assert_eq!(changes2[0].kind, ChangeKind::EventCancelled);

    assert!(
        repo.get_event(&event.id).expect("get").is_none(),
        "ST M-1: cancelled event must be deleted from the DB"
    );

    let (_, changes3) = repo.store_scan(&[], t2()).expect("scan 3: still empty");
    assert!(
        changes3.is_empty(),
        "ST M-1: EventCancelled must be a one-shot signal, not re-emitted: {changes3:?}"
    );
}

/// ST-16: a cancelled event that reappears in a later scan restores its
/// original `first_seen_at` from the tombstone instead of being reset to the
/// reappearance scan time.
#[test]
fn store_scan_reappearing_event_restores_first_seen() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let event = base_event("e1", vec![]);
    let (stored1, _) = repo
        .store_scan(std::slice::from_ref(&event), t0())
        .expect("scan 1: seed");
    let original_first_seen = stored1[0].first_seen_at.expect("first_seen set");

    // scan 2: event disappears → cancelled + tombstone written.
    let (_, changes2) = repo.store_scan(&[], t1()).expect("scan 2: cancel");
    assert_eq!(changes2.len(), 1);
    assert_eq!(changes2[0].kind, ChangeKind::EventCancelled);

    // scan 3: event reappears. first_seen_at must be restored from tombstone.
    let (stored3, changes3) = repo
        .store_scan(std::slice::from_ref(&event), t2())
        .expect("scan 3: reappear");
    assert_eq!(stored3.len(), 1);
    assert_eq!(
        stored3[0].first_seen_at,
        Some(original_first_seen),
        "ST-16: reappearing event must restore original first_seen_at, got {:?} expected {:?}",
        stored3[0].first_seen_at,
        Some(original_first_seen)
    );
    assert_eq!(stored3[0].last_seen_at, Some(t2()));
    // The event re-enters the active set, so EventAdded is emitted (the
    // EventCancelled was a one-shot signal; reappearance is a new addition).
    assert_eq!(changes3.len(), 1);
    assert_eq!(changes3[0].kind, ChangeKind::EventAdded);
}

/// ST-16: after reappearing, the tombstone is removed so a subsequent
/// cancel+reappear cycle works correctly.
#[test]
fn store_scan_reappear_then_cancel_then_reappear() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let event = base_event("e1", vec![]);

    // scan 1: seed (first_seen = t0)
    let (s1, _) = repo
        .store_scan(std::slice::from_ref(&event), t0())
        .expect("scan 1");
    let first_seen_1 = s1[0].first_seen_at.unwrap();

    // scan 2: cancel
    repo.store_scan(&[], t1()).expect("scan 2: cancel");

    // scan 3: reappear (first_seen restored to t0)
    let (s3, _) = repo
        .store_scan(std::slice::from_ref(&event), t2())
        .expect("scan 3");
    assert_eq!(s3[0].first_seen_at, Some(first_seen_1));

    // scan 4: cancel again
    repo.store_scan(&[], t0() + chrono::Duration::hours(72))
        .expect("scan 4: cancel");

    // scan 5: reappear again — first_seen must STILL be t0, not t2.
    let (s5, _) = repo
        .store_scan(
            std::slice::from_ref(&event),
            t0() + chrono::Duration::hours(96),
        )
        .expect("scan 5");
    assert_eq!(
        s5[0].first_seen_at,
        Some(first_seen_1),
        "ST-16: first_seen_at must survive multiple cancel/reappear cycles"
    );
}

/// ST-16: tombstones older than the retention window are purged. An event that
/// reappears after the retention window is treated as genuinely new (first_seen
/// reset to the current scan time).
#[test]
fn store_scan_tombstone_expired_after_retention() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let event = base_event("e1", vec![]);
    let (s1, _) = repo
        .store_scan(std::slice::from_ref(&event), t0())
        .expect("scan 1");
    let original_first_seen = s1[0].first_seen_at.unwrap();

    // scan 2: cancel.
    repo.store_scan(&[], t1()).expect("scan 2: cancel");

    // scan 3: reappear 100 days later — beyond the 90-day retention window.
    // The tombstone has been purged, so the event is treated as brand-new.
    let far_future = t0() + chrono::Duration::days(100);
    let (s3, changes3) = repo
        .store_scan(std::slice::from_ref(&event), far_future)
        .expect("scan 3");
    assert_eq!(
        s3[0].first_seen_at,
        Some(far_future),
        "ST-16: after retention window, reappearing event is brand-new (first_seen reset)"
    );
    assert_eq!(changes3.len(), 1);
    assert_eq!(changes3[0].kind, ChangeKind::EventAdded);
    assert_ne!(s3[0].first_seen_at, Some(original_first_seen));
}

/// store_scan detects EventUpdated on a title change.
#[test]
fn store_scan_event_updated() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let first = event_with_title("e1", "Old Title");
    repo.store_scan(std::slice::from_ref(&first), t0())
        .expect("scan 1");

    let second = event_with_title("e1", "New Title");
    let (_stored, changes) = repo
        .store_scan(std::slice::from_ref(&second), t1())
        .expect("scan 2");

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::EventUpdated);
}

/// store_scan on a read-only repository is rejected with StateError::ReadOnly.
#[test]
fn store_scan_read_only_rejected() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    let event = base_event("e1", vec![]);
    {
        let repo = Repository::open(&db_path).expect("open repo");
        repo.store_scan(std::slice::from_ref(&event), t0())
            .expect("seed");
    }

    let ro = Repository::open_read_only(&db_path).expect("open read-only");
    let err = ro
        .store_scan(std::slice::from_ref(&event), t1())
        .unwrap_err();
    assert!(matches!(err, StateError::ReadOnly), "got {err:?}");
}

/// store_scan persists events that survive a close and reopen.
#[test]
fn store_scan_persists_across_reopen() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let now = t0();

    {
        let repo = Repository::open(&db_path).expect("open repo");
        let event = base_event("e1", vec![]);
        let (stored, changes) = repo
            .store_scan(std::slice::from_ref(&event), now)
            .expect("scan");
        assert_eq!(stored[0].first_seen_at, Some(now));
        assert_eq!(changes.len(), 1);
    }

    let repo = Repository::open(&db_path).expect("reopen repo");
    let retrieved = repo
        .get_event(&EventId("e1".into()))
        .expect("get")
        .expect("present");
    assert_eq!(retrieved.first_seen_at, Some(now));
    assert_eq!(retrieved.last_seen_at, Some(now));
}

/// store_scan handles multiple events with mixed changes in one call.
#[test]
fn store_scan_multiple_events_mixed() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let e1 = base_event("e1", vec![]);
    let e2 = base_event("e2", vec![]);
    repo.store_scan(&[e1.clone(), e2.clone()], t0())
        .expect("scan 1");

    // e1 unchanged, e2 gets media, e3 is new.
    let e2_new = base_event("e2", vec![video("https://youtube.com/v/9")]);
    let e3 = base_event("e3", vec![]);
    let (stored, changes) = repo.store_scan(&[e1, e2_new, e3], t1()).expect("scan 2");

    assert_eq!(stored.len(), 3);
    let kinds: Vec<_> = changes.iter().map(|c| c.kind).collect();
    assert!(
        kinds.contains(&ChangeKind::MediaAdded),
        "e2 media added: {changes:?}"
    );
    assert!(
        kinds.contains(&ChangeKind::EventAdded),
        "e3 added: {changes:?}"
    );
    // e1 unchanged → no record for e1.
    assert!(
        !kinds.contains(&ChangeKind::EventUpdated),
        "no spurious event_updated: {changes:?}"
    );
    // first_seen preserved for e1 and e2.
    let e1_stored = stored.iter().find(|e| e.id.0 == "e1").expect("e1 stored");
    assert_eq!(e1_stored.first_seen_at, Some(t0()));
    let e2_stored = stored.iter().find(|e| e.id.0 == "e2").expect("e2 stored");
    assert_eq!(e2_stored.first_seen_at, Some(t0()));
    let e3_stored = stored.iter().find(|e| e.id.0 == "e3").expect("e3 stored");
    assert_eq!(e3_stored.first_seen_at, Some(t1()));
}

/// store_scan is equivalent to the manual list → detect_changes → store_event
/// pattern but in one atomic transaction (STATE-002 regression).
#[test]
fn store_scan_matches_manual_pattern() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let first = base_event("e1", vec![]);
    repo.store_scan(std::slice::from_ref(&first), t0())
        .expect("scan 1");

    // Manual pattern.
    let prev = repo.list_events().expect("list");
    let second = base_event("e1", vec![video("https://youtube.com/v/2")]);
    let manual_changes = detect_changes(&prev, std::slice::from_ref(&second), t1());

    // store_scan on a fresh repo with the same prev state.
    let dir2 = tempfile::TempDir::new().expect("temp dir 2");
    let db_path2 = dir2.path().join("state.redb");
    let repo2 = Repository::open(&db_path2).expect("open repo 2");
    repo2
        .store_scan(std::slice::from_ref(&first), t0())
        .expect("seed repo 2");
    let (_, scan_changes) = repo2
        .store_scan(std::slice::from_ref(&second), t1())
        .expect("scan 2");

    assert_eq!(manual_changes, scan_changes);
}
