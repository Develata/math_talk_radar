//! Integration tests for the state repository and change detection (§22, §23).
//!
//! Covers STATE-001..004 from `docs/acceptance-cases/06_state_changes.md`.
//! Uses `tempfile::TempDir` so each test gets an isolated redb database file.

use chrono::{DateTime, NaiveDate, Utc};
use radar_core::{
    AccessInfo, DatePrecision, DateTimeOrDate, Event, EventDate, EventId, EventStatus, EventType,
    Location, MediaId, MediaResource, MediaType, OnlineAvailability, PublicAccess, ScoreComponents,
    SourceEvidence, SourceHealth, SourceStatus,
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

/// ADR-0012 (P0-03): when any enabled source has a terminal failure status,
/// the prune step is skipped and EventCancelled change records are suppressed.
/// A scan that returns no events must NOT cancel previously-seen events if the
/// absence is due to a failed source rather than genuine cancellation.
#[test]
fn store_scan_bundle_skips_prune_on_partial_failure() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    // scan 1: seed an event from a healthy source.
    let event = base_event("e1", vec![]);
    let h_ok = health("s1", SourceStatus::Ok, t0());
    repo.store_scan_bundle(
        std::slice::from_ref(&event),
        std::slice::from_ref(&h_ok),
        t0(),
    )
    .expect("scan 1: seed");

    // scan 2: the source now returns HttpError — no events, and the health
    // slice records the failure. The event must survive (not pruned) and no
    // EventCancelled must be emitted.
    let h_err = health("s1", SourceStatus::HttpError, t1());
    let (stored, changes) = repo
        .store_scan_bundle(&[], std::slice::from_ref(&h_err), t1())
        .expect("scan 2: partial failure");
    assert!(stored.is_empty(), "no events stored this scan");
    assert!(
        changes.iter().all(|c| c.kind != ChangeKind::EventCancelled),
        "ADR-0012: EventCancelled must be suppressed on partial failure, got: {changes:?}"
    );

    // The event must still be in the DB.
    assert!(
        repo.get_event(&event.id).expect("get").is_some(),
        "ADR-0012: event must NOT be pruned when a source had a terminal failure"
    );

    // scan 3: the source recovers (Ok) and re-emits the event. No EventAdded
    // should fire (it is the same event id), and first_seen_at must be
    // preserved from scan 1 — proving the tombstone was NOT written in scan 2.
    let h_ok2 = health("s1", SourceStatus::Ok, t2());
    let (stored3, changes3) = repo
        .store_scan_bundle(
            std::slice::from_ref(&event),
            std::slice::from_ref(&h_ok2),
            t2(),
        )
        .expect("scan 3: recovery");
    assert_eq!(stored3.len(), 1);
    assert!(
        changes3.iter().all(|c| c.kind != ChangeKind::EventAdded),
        "ADR-0012: recovering event must not be re-added (no tombstone was written): {changes3:?}"
    );
    assert_eq!(
        stored3[0].first_seen_at,
        Some(t0()),
        "ADR-0012: first_seen_at must be preserved across the failed scan (no tombstone)"
    );
}

/// ADR-0012 (R3-P0-02): `Partial` status means the source's data was
/// truncated (per-source stub cap, global candidate cap, or enrichment
/// failures). Its events are NOT authoritative — absent events may have been
/// dropped rather than genuinely cancelled. The prune guard must suppress
/// cancellation for `Partial` sources just as it does for terminal failures.
#[test]
fn store_scan_bundle_skips_prune_on_partial_status() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    // scan 1: seed an event from a healthy source.
    let event = base_event("e1", vec![]);
    let h_ok = health("s1", SourceStatus::Ok, t0());
    repo.store_scan_bundle(
        std::slice::from_ref(&event),
        std::slice::from_ref(&h_ok),
        t0(),
    )
    .expect("scan 1: seed");

    // scan 2: the source returns Partial (e.g. stubs were truncated). No
    // events this scan, and the health slice records Partial. The event
    // must survive — Partial is not authoritative.
    let h_partial = health("s1", SourceStatus::Partial, t1());
    let (stored, changes) = repo
        .store_scan_bundle(&[], std::slice::from_ref(&h_partial), t1())
        .expect("scan 2: partial status");
    assert!(stored.is_empty());
    assert!(
        changes.iter().all(|c| c.kind != ChangeKind::EventCancelled),
        "R3-P0-02: EventCancelled must be suppressed on Partial status, got: {changes:?}"
    );
    assert!(
        repo.get_event(&event.id).expect("get").is_some(),
        "R3-P0-02: event must NOT be pruned when source status is Partial"
    );
}

/// ADR-0012 (P0-03) complement: when all sources are healthy, the prune step
/// runs as before and EventCancelled is emitted. This guards against the guard
/// being accidentally inverted.
#[test]
fn store_scan_bundle_prunes_when_all_healthy() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open repo");

    let event = base_event("e1", vec![]);
    let h_ok = health("s1", SourceStatus::Ok, t0());
    repo.store_scan_bundle(
        std::slice::from_ref(&event),
        std::slice::from_ref(&h_ok),
        t0(),
    )
    .expect("scan 1: seed");

    // All healthy + empty events → genuine cancel.
    let h_ok2 = health("s1", SourceStatus::Ok, t1());
    let (_, changes) = repo
        .store_scan_bundle(&[], std::slice::from_ref(&h_ok2), t1())
        .expect("scan 2: all healthy, no events");
    assert!(
        changes.iter().any(|c| c.kind == ChangeKind::EventCancelled),
        "ADR-0012: EventCancelled must fire when all sources are healthy, got: {changes:?}"
    );
    assert!(
        repo.get_event(&event.id).expect("get").is_none(),
        "ADR-0012: event must be pruned when all sources are healthy"
    );
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

/// ST-16 / ADR-0011: a v1 database (version=1, no `cancelled_events` table)
/// is migrated in place to v3 by `Repository::open`. The tombstone and
/// change_log tables are created and the version row is bumped — existing
/// events are preserved.
#[test]
fn migrates_v1_to_v3_in_place() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    // Simulate a v1 database: version=1, EVENTS + SOURCE_HEALTH tables, but
    // NO CANCELLED_EVENTS or CHANGE_LOG tables (the v2/v3 additions).
    {
        use radar_state::schema::{EVENTS, SCHEMA_VERSION, SOURCE_HEALTH};
        let db = redb::Database::create(&db_path).expect("create v1 db");
        let txn = db.begin_write().expect("v1 txn");
        {
            let mut vtable = txn.open_table(SCHEMA_VERSION).expect("v1 schema table");
            vtable.insert("version", 1u32).expect("write v1 version");
        }
        let _ = txn.open_table(EVENTS).expect("v1 events table");
        let _ = txn.open_table(SOURCE_HEALTH).expect("v1 health table");
        txn.commit().expect("v1 commit");
    }

    // Open with current binary — should forward-migrate v1→v3.
    let repo = Repository::open(&db_path).expect("migrate v1 to v3");
    assert_eq!(repo.schema_version().expect("version"), 3);

    // The tombstone table must exist: exercise it by storing, cancelling, and
    // reappearing an event. If the table were missing, the cancel step would
    // fail and first_seen_at would not be restored on reappearance.
    let event = base_event("e1", vec![]);
    repo.store_scan(std::slice::from_ref(&event), t0())
        .expect("seed");
    repo.store_scan(&[], t1()).expect("cancel");
    let (stored, _) = repo
        .store_scan(std::slice::from_ref(&event), t2())
        .expect("reappear");
    assert_eq!(
        stored[0].first_seen_at,
        Some(t0()),
        "first_seen_at restored from tombstone after v1 to v3 migration"
    );
}

/// A database with a schema version newer than the binary is refused
/// (backward-incompatible downgrade protection).
#[test]
fn refuses_to_open_newer_schema_version() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    {
        use radar_state::schema::SCHEMA_VERSION;
        let db = redb::Database::create(&db_path).expect("create future db");
        let txn = db.begin_write().expect("txn");
        {
            let mut vtable = txn.open_table(SCHEMA_VERSION).expect("schema table");
            vtable
                .insert("version", 999u32)
                .expect("write future version");
        }
        txn.commit().expect("commit");
    }

    let err = Repository::open(&db_path).unwrap_err();
    assert!(
        matches!(
            err,
            StateError::Schema {
                expected: 3,
                found: 999
            }
        ),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// ADR-0011 tests: store_scan_bundle, source-health history, change log,
// retention purge, v2→v3 migration.
// ---------------------------------------------------------------------------

fn health(source: &str, status: SourceStatus, at: DateTime<Utc>) -> SourceHealth {
    SourceHealth {
        source: source.into(),
        status,
        duration_ms: 100,
        requests: 5,
        events: 10,
        recorded_at: Some(at),
    }
}

/// TXN-1 (ADR-0011 §6): store_scan_bundle atomically persists events, change
/// records, and source-health observations in ONE transaction. Reopening the
/// repo must show all three — no partial write.
#[test]
fn bundle_atomicity_persists_events_changes_and_health() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    {
        let repo = Repository::open(&db_path).expect("open");
        let event = base_event("e1", vec![]);
        let h = health("s1", SourceStatus::Ok, t0());
        let (stored, changes) = repo
            .store_scan_bundle(std::slice::from_ref(&event), std::slice::from_ref(&h), t0())
            .expect("bundle");
        assert_eq!(stored.len(), 1);
        assert_eq!(changes.len(), 1, "EventAdded change expected");
        assert_eq!(changes[0].kind, ChangeKind::EventAdded);
    }

    // Reopen — all three must be present.
    let repo2 = Repository::open(&db_path).expect("reopen");
    let events = repo2.list_events().expect("list events");
    assert_eq!(events.len(), 1, "event must survive reopen");
    let health_history = repo2.list_source_health("s1").expect("list health");
    assert_eq!(health_history.len(), 1, "health record must survive reopen");
    assert_eq!(health_history[0].source, "s1");
    let change_history = repo2
        .list_changes(DateTime::from_timestamp(0, 0).unwrap())
        .expect("list changes");
    assert!(
        change_history
            .iter()
            .any(|c| c.kind == ChangeKind::EventAdded),
        "change record must survive reopen: {change_history:?}"
    );
}

/// ADR-0011 §1/§2: source-health history accumulates per-scan records.
/// Two scans of the same source must produce two distinct health records,
/// retrievable in chronological order via list_source_health.
#[test]
fn source_health_history_accumulates_across_scans() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open");

    let h0 = health("s1", SourceStatus::Ok, t0());
    let h1 = health("s1", SourceStatus::Partial, t1());
    let event = base_event("e1", vec![]);
    repo.store_scan_bundle(
        std::slice::from_ref(&event),
        std::slice::from_ref(&h0),
        t0(),
    )
    .expect("scan 1");
    repo.store_scan_bundle(
        std::slice::from_ref(&event),
        std::slice::from_ref(&h1),
        t1(),
    )
    .expect("scan 2");

    let history = repo.list_source_health("s1").expect("list");
    assert_eq!(
        history.len(),
        2,
        "two scans must produce two health records"
    );
    assert_eq!(history[0].recorded_at, Some(t0()), "oldest first");
    assert_eq!(history[1].recorded_at, Some(t1()), "newest second");
    assert_eq!(history[0].status, SourceStatus::Ok);
    assert_eq!(history[1].status, SourceStatus::Partial);
}

/// ADR-0011 §3 (R9-H08): change records are persisted to CHANGE_LOG and
/// survive a reopen. Media history must not be silently lost (§65).
#[test]
fn change_log_persists_media_added_across_reopen() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    {
        let repo = Repository::open(&db_path).expect("open");
        let event = base_event("e1", vec![]);
        repo.store_scan_bundle(std::slice::from_ref(&event), &[], t0())
            .expect("seed");
        let event_with_video = base_event("e1", vec![video("https://youtube.com/v/1")]);
        repo.store_scan_bundle(std::slice::from_ref(&event_with_video), &[], t1())
            .expect("add video");
    }

    let repo2 = Repository::open(&db_path).expect("reopen");
    let changes = repo2
        .list_changes(DateTime::from_timestamp(0, 0).unwrap())
        .expect("list changes");
    assert!(
        changes.iter().any(|c| c.kind == ChangeKind::MediaAdded),
        "MediaAdded must survive reopen: {changes:?}"
    );
}

/// Regression: multiple same-kind change records on the same event in one
/// scan must all survive — the composite CHANGE_LOG key includes `detail`
/// (the URL/talk-id/speaker-name) so records do not collide and overwrite
/// each other. Before the fix, two MediaAdded on the same event in one scan
/// shared an identical key and only the last survived.
#[test]
fn change_log_preserves_multiple_same_kind_records() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    {
        let repo = Repository::open(&db_path).expect("open");
        let event = base_event("e1", vec![]);
        repo.store_scan_bundle(std::slice::from_ref(&event), &[], t0())
            .expect("seed");
        let event_with_two_videos = base_event(
            "e1",
            vec![
                video("https://youtube.com/v/1"),
                video("https://youtube.com/v/2"),
            ],
        );
        repo.store_scan_bundle(std::slice::from_ref(&event_with_two_videos), &[], t1())
            .expect("add two videos in one scan");
    }

    let repo2 = Repository::open(&db_path).expect("reopen");
    let changes = repo2
        .list_changes(DateTime::from_timestamp(0, 0).unwrap())
        .expect("list changes");
    let media_added: Vec<_> = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::MediaAdded)
        .collect();
    assert_eq!(
        media_added.len(),
        2,
        "both MediaAdded records must survive (one per URL), got: {media_added:?}"
    );
    let details: Vec<_> = media_added
        .iter()
        .map(|c| c.detail.as_deref().unwrap_or(""))
        .collect();
    assert!(
        details.contains(&"https://youtube.com/v/1")
            && details.contains(&"https://youtube.com/v/2"),
        "both URLs must be present as detail, got: {details:?}"
    );
}

/// ADR-0011 §7: retention purge. Health records and change records older
/// than RETENTION_DAYS (90) are purged during store_scan_bundle. After
/// advancing time 91 days, the old records must be gone.
#[test]
fn retention_purges_expired_health_and_changes() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");
    let repo = Repository::open(&db_path).expect("open");

    let event = base_event("e1", vec![]);
    let h = health("s1", SourceStatus::Ok, t0());
    repo.store_scan_bundle(std::slice::from_ref(&event), std::slice::from_ref(&h), t0())
        .expect("seed at t0");

    // 91 days later: the old health record and change record are past
    // retention. A new scan must purge them. Store the SAME event (not empty)
    // so no EventCancelled is produced — the only old change is EventAdded
    // from t0, which must be purged.
    let t_old = t0() + chrono::Duration::days(91);
    let h_new = health("s1", SourceStatus::Ok, t_old);
    repo.store_scan_bundle(
        std::slice::from_ref(&event),
        std::slice::from_ref(&h_new),
        t_old,
    )
    .expect("scan at t+91d");

    let history = repo.list_source_health("s1").expect("list health");
    assert_eq!(
        history.len(),
        1,
        "old health record must be purged, only the new one remains"
    );
    assert_eq!(history[0].recorded_at, Some(t_old));

    let changes = repo
        .list_changes(DateTime::from_timestamp(0, 0).unwrap())
        .expect("list changes");
    assert!(
        changes.iter().all(|c| c.detected_at >= t_old),
        "change records older than retention window must be purged, got: {changes:?}"
    );
}

/// ADR-0011 §5: v2→v3 migration re-keys legacy SOURCE_HEALTH rows from bare
/// source id to composite "{source}\x00{recorded_at}". On real v2 databases
/// the table is empty (scan path never wrote it); this test simulates the
/// defensive case where a legacy row exists.
#[test]
fn migrates_v2_to_v3_rekeys_legacy_source_health() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    // Simulate a v2 database with a legacy SOURCE_HEALTH row keyed by bare
    // source id (no composite key, no recorded_at in the serialized value).
    {
        use radar_state::schema::{CANCELLED_EVENTS, EVENTS, SCHEMA_VERSION, SOURCE_HEALTH};
        let db = redb::Database::create(&db_path).expect("create v2 db");
        let txn = db.begin_write().expect("v2 txn");
        {
            let mut vtable = txn.open_table(SCHEMA_VERSION).expect("v2 schema table");
            vtable.insert("version", 2u32).expect("write v2 version");
        }
        let _ = txn.open_table(EVENTS).expect("v2 events table");
        let _ = txn.open_table(SOURCE_HEALTH).expect("v2 health table");
        let _ = txn
            .open_table(CANCELLED_EVENTS)
            .expect("v2 tombstone table");

        // Insert a legacy health row: key = bare "s1", value = SourceHealth
        // serialized WITHOUT recorded_at (serde skip_serializing_if omits None).
        let legacy = SourceHealth {
            source: "s1".into(),
            status: SourceStatus::Ok,
            duration_ms: 50,
            requests: 3,
            events: 7,
            recorded_at: None,
        };
        let bytes = serde_json::to_vec(&legacy).expect("serialize legacy");
        {
            let mut health_table = txn.open_table(SOURCE_HEALTH).expect("health table");
            health_table
                .insert("s1", bytes.as_slice())
                .expect("insert legacy");
        }
        txn.commit().expect("v2 commit");
    }

    // Open with current binary — should forward-migrate v2→v3 and re-key.
    let repo = Repository::open(&db_path).expect("migrate v2 to v3");
    assert_eq!(repo.schema_version().expect("version"), 3);

    // The legacy row must have been re-keyed to composite and stamped with
    // recorded_at (= migration time, not None).
    let history = repo.list_source_health("s1").expect("list health");
    assert_eq!(history.len(), 1, "legacy row must survive migration");
    assert!(
        history[0].recorded_at.is_some(),
        "recorded_at must be stamped during migration, got {:?}",
        history[0].recorded_at
    );
    assert_eq!(history[0].source, "s1");
    assert_eq!(history[0].duration_ms, 50);
}

/// R3-P1-02: a malformed legacy SOURCE_HEALTH row must abort the migration
/// without bumping the schema version. The previous code silently skipped
/// malformed rows and still committed v3, making them unreachable (the v3
/// read path skips non-composite keys).
#[test]
fn migrates_v2_to_v3_fails_on_malformed_legacy_row() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = dir.path().join("state.redb");

    {
        use radar_state::schema::{CANCELLED_EVENTS, EVENTS, SCHEMA_VERSION, SOURCE_HEALTH};
        let db = redb::Database::create(&db_path).expect("create v2 db");
        let txn = db.begin_write().expect("v2 txn");
        {
            let mut vtable = txn.open_table(SCHEMA_VERSION).expect("v2 schema table");
            vtable.insert("version", 2u32).expect("write v2 version");
        }
        let _ = txn.open_table(EVENTS).expect("v2 events table");
        let _ = txn.open_table(SOURCE_HEALTH).expect("v2 health table");
        let _ = txn
            .open_table(CANCELLED_EVENTS)
            .expect("v2 tombstone table");

        // Insert a malformed row: bare key "s1", value is NOT valid JSON.
        {
            let mut health_table = txn.open_table(SOURCE_HEALTH).expect("health table");
            health_table
                .insert("s1", b"not valid json".as_slice())
                .expect("insert malformed");
        }
        txn.commit().expect("v2 commit");
    }

    // Migration must fail — the malformed row cannot be deserialized.
    let result = Repository::open(&db_path);
    assert!(
        result.is_err(),
        "R3-P1-02: migration must fail on malformed legacy row"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, StateError::Migration(ref msg) if msg.contains("malformed")),
        "R3-P1-02: error must be StateError::Migration, got: {err:?}"
    );

    // The schema version must still be 2 — the transaction rolled back.
    {
        use radar_state::schema::SCHEMA_VERSION;
        let db = redb::Database::open(&db_path).expect("reopen db");
        let txn = db.begin_read().expect("read txn");
        let vtable = txn.open_table(SCHEMA_VERSION).expect("schema table");
        let version = vtable.get("version").expect("get version").unwrap().value();
        assert_eq!(
            version, 2,
            "R3-P1-02: schema version must stay at 2 after failed migration"
        );
    }
}
