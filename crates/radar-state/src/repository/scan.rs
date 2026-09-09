//! Owned scan writes, v4 history, and the all-healthy absence guard (ADR-0016).
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use radar_core::{Event, EventId, SourceHealth, SourceStatus};
use redb::ReadableTable;

use super::{CancelledEventTombstone, RETENTION_DAYS, Repository, StateError};
use crate::changes::{ChangeRecord, detect_changes_when};
use crate::schema::{
    CANCELLED_EVENTS, CHANGE_LOG, EVENTS, SOURCE_HEALTH, change_log_key, source_health_key,
    timestamp_key,
};

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
        self.store_scan_bundle_owned(events, &[], now, &HashMap::new())
    }

    /// Compatibility entrypoint for borrowed observations and source health.
    /// Events, change log and source-health history commit atomically.
    pub fn store_scan_bundle(
        &self,
        events: &[Event],
        source_health: &[SourceHealth],
        now: DateTime<Utc>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        self.store_scan_bundle_owned(events.to_vec(), source_health, now, &HashMap::new())
    }

    /// Stamp the caller's vector in place and persist the scan in one transaction.
    /// Any non-Ok health entry blocks all absence pruning and EventCancelled
    /// records. Empty health asserts a complete snapshot. Scan-local dedup
    /// aliases transfer existing history without adding a new identity rule.
    /// V4 health/change keys and 90-day retention share the same transaction.
    pub fn store_scan_bundle_owned(
        &self,
        mut events: Vec<Event>,
        source_health: &[SourceHealth],
        now: DateTime<Utc>,
        aliases: &HashMap<EventId, EventId>,
    ) -> Result<(Vec<Event>, Vec<ChangeRecord>), StateError> {
        if self.read_only {
            return Err(StateError::ReadOnly);
        }
        let all_authoritative = source_health.iter().all(|h| h.status == SourceStatus::Ok);
        let authoritative: HashSet<String> = source_health
            .iter()
            .filter(|h| h.status == SourceStatus::Ok)
            .map(|h| h.source.clone())
            .collect();
        let txn = self.db.begin_write()?;
        let changes = {
            let mut table = txn.open_table(EVENTS)?;
            let mut tombstones = txn.open_table(CANCELLED_EVENTS)?;
            let mut health_table = txn.open_table(SOURCE_HEALTH)?;
            let mut change_log = txn.open_table(CHANGE_LOG)?;
            let current_ids: HashSet<&EventId> = if aliases.is_empty() {
                HashSet::new()
            } else {
                events.iter().map(|e| &e.id).collect()
            };
            let canonical_id = |id: &EventId| {
                aliases
                    .get(id)
                    .filter(|target| !current_ids.contains(id) && current_ids.contains(target))
                    .cloned()
                    .unwrap_or_else(|| id.clone())
            };
            let mut previous = Vec::new();
            let mut obsolete_ids = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let mut event: Event = serde_json::from_slice(value.value())?;
                if let Some(canonical) = aliases.get(&event.id).filter(|target| {
                    !current_ids.contains(&event.id) && current_ids.contains(target)
                }) {
                    obsolete_ids.push(event.id.clone());
                    event.id = canonical.clone();
                }
                previous.push(event);
            }
            // The normal path is already in table key order. Only an actual
            // alias transfer needs coalescing and another sort.
            if !obsolete_ids.is_empty() {
                let mut grouped = HashMap::new();
                for mut event in previous {
                    if let Some(other) = grouped.remove(&event.id) {
                        event = radar_core::dedup::merge_events(other, event);
                    }
                    grouped.insert(event.id.clone(), event);
                }
                previous = grouped.into_values().collect();
                previous.sort_by(|a, b| a.id.0.cmp(&b.id.0));
            }

            let cutoff = now - chrono::Duration::days(RETENTION_DAYS);
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
                if !all_authoritative && let Some(prev) = prev {
                    retain_uncertain_evidence(event, prev, &authoritative);
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
            let can_cancel = |_: &Event| all_authoritative;
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
            // ADR-0011 §3 (R9-H08): persist change records to CHANGE_LOG so
            // media history and change signals survive a restart (§65).
            //
            // Key shape:
            // `{fixed_detected_at}\x00{event_id}\x00{kind}\x00{detail_digest}`.
            // The timestamp is fixed-width UTC RFC3339 so lexical range order
            // equals time order within a second. The BLAKE3 detail digest keeps
            // the key bounded while preserving multiple same-kind records on
            // one event/scan (MediaAdded URLs, talks, speakers, and so on).
            for record in &changes {
                let key = change_log_key(record);
                let bytes = serde_json::to_vec(record)?;
                change_log.insert(key.as_str(), bytes.as_slice())?;
            }

            // ADR-0011 §1/§2 (R9-M06/B06): persist source-health observations
            // to SOURCE_HEALTH under composite key for per-scan history.
            // Defensive: stamp recorded_at = now if the caller left it None.
            for h in source_health {
                let (ts, bytes) = match h.recorded_at {
                    Some(existing) => (existing, serde_json::to_vec(h)?),
                    None => {
                        let mut stamped = h.clone();
                        stamped.recorded_at = Some(now);
                        (now, serde_json::to_vec(&stamped)?)
                    }
                };
                let key = source_health_key(&h.source, ts);
                health_table.insert(key.as_str(), bytes.as_slice())?;
            }

            // ADR-0011 §7: purge expired SOURCE_HEALTH and CHANGE_LOG records
            // (older than RETENTION_DAYS). Same transaction as the writes.
            // Key-based expiry: the composite keys encode the timestamp, so we
            // parse it from the key string instead of deserializing the full
            // value — O(n) iteration but zero serde cost per row, and a
            // corrupt value can't crash the purge.
            let cutoff_rfc3339 = timestamp_key(cutoff);
            let mut expired_health_keys: Vec<String> = Vec::new();
            for entry in health_table.iter()? {
                let (key, _value) = entry?;
                let key_str = key.value();
                if let Some(idx) = key_str.find('\u{0}') {
                    let ts = &key_str[idx + 1..];
                    if ts < cutoff_rfc3339.as_str() {
                        expired_health_keys.push(key_str.to_string());
                    }
                }
            }
            for key in expired_health_keys {
                health_table.remove(key.as_str())?;
            }

            // CHANGE_LOG key starts with `{detected_at_rfc3339}\x00...`, so
            // range(..cutoff) yields exactly the expired records — O(log n +
            // expired), zero deserialization.
            let mut expired_change_keys: Vec<String> = Vec::new();
            for entry in change_log.range(..cutoff_rfc3339.as_str())? {
                let (key, _value) = entry?;
                expired_change_keys.push(key.value().to_string());
            }
            for key in expired_change_keys {
                change_log.remove(key.as_str())?;
            }

            changes
        };
        txn.commit()?;
        Ok((events, changes))
    }
}

fn previous_observation_complete(event: &Event, sources: &HashSet<String>) -> bool {
    !event.sources.is_empty() && event.sources.iter().all(|s| sources.contains(&s.source_id))
}

/// An incomplete observation must not erase failed-source provenance or
/// turn missing detail into MediaRemoved. This does not authorize cancellation.
fn retain_uncertain_evidence(current: &mut Event, previous: &Event, authority: &HashSet<String>) {
    if previous_observation_complete(previous, authority) {
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
