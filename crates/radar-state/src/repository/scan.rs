//! Transactional scan ownership and per-event absence authority. The stored
//! Event JSON and v2 tables remain unchanged.
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use radar_core::{Event, EventId};
use redb::ReadableTable;

use super::{CancelledEventTombstone, Repository, StateError, TOMBSTONE_RETENTION_DAYS};
use crate::changes::{ChangeRecord, detect_changes_when};
use crate::schema::{CANCELLED_EVENTS, EVENTS};

impl Repository {
    /// Compatibility entrypoint for callers supplying a complete snapshot.
    /// The scan pipeline should use the owned, source-authority-aware method.
    pub fn store_scan(
        &self,
        events: &[Event],
        now: DateTime<Utc>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        self.store_scan_owned(events.to_vec(), now)
    }

    /// Consume and stamp a complete snapshot in place (no current-corpus clone).
    pub fn store_scan_owned(
        &self,
        events: Vec<Event>,
        now: DateTime<Utc>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        self.store_scan_impl(events, now, None, &HashMap::new())
    }

    /// Consume current observations and return the same stamped vector. Only
    /// supplied authoritative sources can establish absence. Aliases are the
    /// current dedup pass's input-ID → representative-ID mapping, used solely
    /// to transfer existing history transactionally (never as a new match rule).
    pub fn store_scan_with_authority(
        &self,
        events: Vec<Event>,
        now: DateTime<Utc>,
        authoritative: &HashSet<String>,
        aliases: &HashMap<EventId, EventId>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        self.store_scan_impl(events, now, Some(authoritative), aliases)
    }

    fn store_scan_impl(
        &self,
        mut events: Vec<Event>,
        now: DateTime<Utc>,
        authoritative: Option<&HashSet<String>>,
        aliases: &HashMap<EventId, EventId>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        if self.read_only {
            return Err(StateError::ReadOnly);
        }
        let txn = self.db.begin_write()?;
        let changes = {
            let mut table = txn.open_table(EVENTS)?;
            let mut tombstones = txn.open_table(CANCELLED_EVENTS)?;
            let current_ids: HashSet<&EventId> = events.iter().map(|e| &e.id).collect();
            let canonical_id = |id: &EventId| {
                aliases
                    .get(id)
                    .filter(|target| !current_ids.contains(id) && current_ids.contains(target))
                    .cloned()
                    .unwrap_or_else(|| id.clone())
            };
            let mut previous: HashMap<EventId, Event> = HashMap::new();
            let mut obsolete_ids = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let mut event: Event = serde_json::from_slice(value.value())?;
                let canonical = canonical_id(&event.id);
                if canonical != event.id {
                    obsolete_ids.push(event.id.clone());
                    event.id = canonical;
                }
                if let Some(other) = previous.remove(&event.id) {
                    event = radar_core::dedup::merge_events(other, event);
                }
                previous.insert(event.id.clone(), event);
            }
            // Tables iterate by key, but alias coalescing uses a HashMap.
            // Restore stable ordering before any evidence merge or change output.
            let mut previous: Vec<Event> = previous.into_values().collect();
            previous.sort_by(|a, b| a.id.0.cmp(&b.id.0));

            let cutoff = now - chrono::Duration::days(TOMBSTONE_RETENTION_DAYS);
            let mut tombstone_first: HashMap<EventId, DateTime<Utc>> = HashMap::new();
            let mut obsolete_tombstones = Vec::new();
            for entry in tombstones.iter()? {
                let (key, value) = entry?;
                let id = EventId(key.value().to_owned());
                let tombstone: CancelledEventTombstone = serde_json::from_slice(value.value())?;
                let canonical = canonical_id(&id);
                if tombstone.cancelled_at < cutoff || canonical != id {
                    obsolete_tombstones.push(id);
                }
                if tombstone.cancelled_at >= cutoff {
                    tombstone_first
                        .entry(canonical)
                        .and_modify(|first| *first = (*first).min(tombstone.first_seen_at))
                        .or_insert(tombstone.first_seen_at);
                }
            }
            let previous_by_id: HashMap<&EventId, &Event> =
                previous.iter().map(|e| (&e.id, e)).collect();
            for event in &mut events {
                let prev = previous_by_id.get(&event.id).copied();
                if let (Some(prev), Some(authority)) = (prev, authoritative) {
                    retain_uncertain_evidence(event, prev, authority);
                }
                event.first_seen_at = Some(
                    prev.and_then(|p| p.first_seen_at)
                        .into_iter()
                        .chain(tombstone_first.get(&event.id).copied())
                        .min()
                        .unwrap_or(now),
                );
                event.last_seen_at = Some(now);
                let bytes = serde_json::to_vec(event)?;
                table.insert(event.id.0.as_str(), bytes.as_slice())?;
                tombstones.remove(event.id.0.as_str())?;
            }
            let can_cancel = |event: &Event| absence_is_authoritative(event, authoritative);
            let changes = detect_changes_when(&previous, &events, now, can_cancel);
            let current_ids: HashSet<&EventId> = events.iter().map(|e| &e.id).collect();
            for prev in &previous {
                if !current_ids.contains(&prev.id) && can_cancel(prev) {
                    if let Some(first_seen_at) = prev.first_seen_at {
                        let bytes = serde_json::to_vec(&CancelledEventTombstone {
                            first_seen_at,
                            cancelled_at: now,
                        })?;
                        tombstones.insert(prev.id.0.as_str(), bytes.as_slice())?;
                    }
                    table.remove(prev.id.0.as_str())?;
                }
            }
            for id in obsolete_ids {
                table.remove(id.0.as_str())?;
            }
            for id in obsolete_tombstones {
                tombstones.remove(id.0.as_str())?;
            }
            changes
        };
        txn.commit()?;
        Ok((events, changes))
    }
}

fn absence_is_authoritative(event: &Event, authoritative: Option<&HashSet<String>>) -> bool {
    authoritative.is_none_or(|sources| {
        !event.sources.is_empty() && event.sources.iter().all(|s| sources.contains(&s.source_id))
    })
}

/// An incomplete observation must not erase a failed source's provenance (and
/// thereby lose its veto next scan), or turn missing detail into MediaRemoved.
fn retain_uncertain_evidence(current: &mut Event, previous: &Event, authority: &HashSet<String>) {
    if absence_is_authoritative(previous, Some(authority)) {
        return;
    }
    let mut sources: HashSet<_> = current
        .sources
        .iter()
        .map(|s| (s.source_id.clone(), s.source_url.clone()))
        .collect();
    for source in &previous.sources {
        if !authority.contains(&source.source_id)
            && sources.insert((source.source_id.clone(), source.source_url.clone()))
        {
            current.sources.push(source.clone());
        }
    }
    // A merged resource records only one source. With incomplete event
    // coverage we cannot prove another supporting source lost that resource.
    let mut media: HashSet<_> = current.media.iter().map(|m| m.url.clone()).collect();
    for medium in &previous.media {
        if media.insert(medium.url.clone()) {
            current.media.push(medium.clone());
        }
    }
    let mut talks: HashSet<_> = current.talks.iter().map(|t| t.id.clone()).collect();
    for talk in &previous.talks {
        if talks.insert(talk.id.clone()) {
            current.talks.push(talk.clone());
        }
    }
}
