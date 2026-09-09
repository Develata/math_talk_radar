//! Cross-source deduplication (§25, §47).
//!
//! Conservative deterministic dedup. Signals tried in priority order:
//! canonical URL → source-declared canonical ID → normalized
//! title+date+organizer → normalized title+date+location. We prefer keeping a
//! suspected duplicate over merging two distinct events: a wrong merge is a
//! release blocker (§47). Fuzzy/semantic dedup is deferred.
//!
//! Determinism: identical inputs produce identical merge decisions. No clocks,
//! no randomness, no order-dependent ties (we sort before merging).
use crate::model::Event;
use std::collections::{BTreeSet, HashMap};

mod identity;
mod merge;

use identity::{DedupKeys, IdentityKey, native_identity_keys, scalar_identity_keys};
pub use identity::{DedupSignal, are_duplicates, duplicate_signal};
pub use merge::merge_events;
#[cfg(test)]
use merge::merge_in_place;
use merge::{MergeIndex, canonical_cmp};

/// A key can belong to multiple clusters after a representative acquires new
/// fields. Keep their ordered IDs so removing an obsolete key reveals the next
/// earliest cluster instead of losing it. No iteration order of HashMap leaks.
#[derive(Default)]
struct DedupIndex {
    clusters: HashMap<IdentityKey, BTreeSet<usize>>,
    #[cfg(test)]
    lookups: usize,
    #[cfg(test)]
    updates: usize,
}

impl DedupIndex {
    fn first_match(&mut self, keys: &[IdentityKey]) -> Option<usize> {
        keys.iter()
            .filter_map(|key| {
                #[cfg(test)]
                {
                    self.lookups += 1;
                }
                self.clusters.get(key).and_then(|ids| ids.first()).copied()
            })
            .min()
    }

    fn insert(&mut self, id: usize, keys: &[IdentityKey]) {
        for key in keys {
            #[cfg(test)]
            {
                self.updates += 1;
            }
            self.clusters.entry(key.clone()).or_default().insert(id);
        }
    }

    fn remove(&mut self, id: usize, keys: &[IdentityKey]) {
        for key in keys {
            #[cfg(test)]
            {
                self.updates += 1;
            }
            if let std::collections::hash_map::Entry::Occupied(mut entry) =
                self.clusters.entry(key.clone())
            {
                entry.get_mut().remove(&id);
                if entry.get().is_empty() {
                    entry.remove();
                }
            }
        }
    }
}

/// Stable canonical ordering followed by first-matching-cluster deduplication.
/// Expected O(n log n + k log n), where k counts processed identity keys,
/// including representative key updates. Does not compute transitive closure.
pub fn dedup_events(events: Vec<Event>) -> Vec<Event> {
    dedup_indexed(events, false, &mut DedupIndex::default()).0
}

/// Also report input IDs replaced by a different canonical representative.
/// State uses these aliases to preserve history after representative correction;
/// they are scan-local and do not change the persisted schema or ID hash.
pub fn dedup_events_with_aliases(
    events: Vec<Event>,
) -> (Vec<Event>, HashMap<crate::EventId, crate::EventId>) {
    dedup_indexed(events, true, &mut DedupIndex::default())
}

fn dedup_indexed(
    mut events: Vec<Event>,
    aliases: bool,
    index: &mut DedupIndex,
) -> (Vec<Event>, HashMap<crate::EventId, crate::EventId>) {
    events.sort_by(canonical_cmp);
    let mut clusters = Vec::with_capacity(events.len());
    let mut cluster_keys: Vec<Vec<IdentityKey>> = Vec::with_capacity(events.len());
    let mut origins = Vec::new();
    let mut merge_indices: Vec<Option<MergeIndex>> = Vec::with_capacity(events.len());
    for event in events {
        let keys = DedupKeys::from_event(&event).identity_keys();
        let position = index.first_match(&keys).unwrap_or(clusters.len());
        if aliases {
            origins.push((event.id.clone(), position));
        }
        if position == clusters.len() {
            index.insert(position, &keys);
            cluster_keys.push(
                keys.into_iter()
                    .filter(|key| !matches!(key, IdentityKey::Native(..)))
                    .collect(),
            );
            clusters.push(event);
            merge_indices.push(None);
        } else {
            let keep = &mut clusters[position];
            let replacing = canonical_cmp(keep, &event).is_gt();
            if replacing {
                // Rare equal-ID tie replacement may discard old native IDs under
                // existing source-URL conflict rules; rebuild this cluster only.
                index.remove(position, &native_identity_keys(keep).collect::<Vec<_>>());
            }
            let old_sources = keep.sources.len();
            merge_indices[position]
                .get_or_insert_with(|| MergeIndex::new(keep))
                .merge(keep, event);
            let keys = scalar_identity_keys(keep);
            let old = &cluster_keys[position];
            for key in old.iter().filter(|key| !keys.contains(key)) {
                index.remove(position, std::slice::from_ref(key));
            }
            for key in keys.iter().filter(|key| !old.contains(key)) {
                index.insert(position, std::slice::from_ref(key));
            }
            cluster_keys[position] = keys;
            let start = if replacing { 0 } else { old_sources };
            for source in &keep.sources[start..] {
                if let Some(native) = &source.native_id {
                    index.insert(
                        position,
                        &[IdentityKey::Native(
                            source.source_id.clone(),
                            native.clone(),
                        )],
                    );
                }
            }
        }
    }
    let aliases = origins
        .into_iter()
        .filter_map(|(id, pos)| {
            let canonical = &clusters[pos].id;
            (id != *canonical).then(|| (id, canonical.clone()))
        })
        .collect();
    (clusters, aliases)
}

#[cfg(test)]
fn reference_dedup_events(events: Vec<Event>) -> Vec<Event> {
    // Stable sort by id so the cluster representative is deterministic
    // regardless of input order.
    let mut sorted: Vec<Event> = events;
    sorted.sort_by(canonical_cmp);

    let mut clusters: Vec<Event> = Vec::with_capacity(sorted.len());
    let mut cluster_keys: Vec<DedupKeys> = Vec::with_capacity(sorted.len());
    for ev in sorted {
        let incoming = DedupKeys::from_event(&ev);
        let mut current: Option<(Event, DedupKeys)> = Some((ev, incoming));
        for (rep, rep_keys) in clusters.iter_mut().zip(cluster_keys.iter_mut()) {
            let Some((remaining, remaining_keys)) = current.take() else {
                break;
            };
            if rep_keys.duplicate_signal(&remaining_keys).is_some() {
                merge_in_place(rep, remaining);
                *rep_keys = DedupKeys::from_event(rep);
            } else {
                current = Some((remaining, remaining_keys));
            }
        }
        if let Some((remaining, keys)) = current {
            clusters.push(remaining);
            cluster_keys.push(keys);
        }
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::identity::domain_key;
    use super::*;
    use crate::date::{DatePrecision, EventDate};
    use crate::model::Location;
    use crate::model::{
        AccessInfo, EventId, EventStatus, EventType, OnlineAvailability, PublicAccess,
        SourceEvidence, Talk, TalkId,
    };
    use crate::normalize::canonicalize_url;
    use crate::people::{PersonHit, PersonRole};
    use crate::topics::TopicMatch;
    use url::Url;

    fn src(source_id: &str, url: &str, native_id: Option<&str>) -> SourceEvidence {
        SourceEvidence {
            source_id: source_id.to_string(),
            source_url: Url::parse(url).unwrap(),
            evidence: None,
            captured_at: None,
            native_id: native_id.map(str::to_string),
        }
    }

    fn event(
        id: &str,
        title: &str,
        url: Option<&str>,
        start: Option<chrono::NaiveDate>,
        sources: Vec<SourceEvidence>,
    ) -> Event {
        Event {
            id: EventId(id.to_string()),
            title: title.to_string(),
            url: url.map(|u| Url::parse(u).unwrap()),
            event_type: EventType::Conference,
            status: EventStatus::Unknown,
            date: EventDate {
                start: start.map(crate::date::DateTimeOrDate::Date),
                end: None,
                timezone: None,
                original_text: String::new(),
                precision: start.map_or(DatePrecision::Unknown, |_| DatePrecision::Day),
            },
            location: None,
            description: None,
            topics: Vec::new(),
            people: Vec::new(),
            talks: Vec::new(),
            media: Vec::new(),
            access: AccessInfo {
                access: PublicAccess::Unknown,
                online: OnlineAvailability::Unknown,
            },
            sources,
            score: 0.0,
            score_components: crate::ranking::ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        }
    }

    #[test]
    fn ranking_preferences_do_not_change_canonical_identity() {
        use crate::ranking::{InterestWeights, score_event};
        use std::collections::HashMap;
        let mut a = event(
            "a",
            "Algebra",
            Some("https://x.com/e"),
            None,
            vec![src("a", "https://x.com/e", Some("1"))],
        );
        let mut b = event(
            "b",
            "Geometry",
            Some("https://x.com/e#top"),
            None,
            vec![src("b", "https://y.com/e", Some("2"))],
        );
        a.description = Some("A".into());
        b.description = Some("B".into());
        for (ev, topic) in [(&mut a, "algebra"), (&mut b, "geometry")] {
            ev.topics = vec![TopicMatch {
                topic_id: topic.into(),
                canonical_name: topic.into(),
                matched_text: topic.into(),
                confidence: 1.0,
            }];
        }
        let run = |weights: &str| {
            let interests = InterestWeights::parse(weights).unwrap();
            let mut inputs = vec![a.clone(), b.clone()];
            for e in &mut inputs {
                (e.score, e.score_components, e.rank_reasons) =
                    score_event(e, &HashMap::new(), Some(&interests));
            }
            let mut out = dedup_events(inputs);
            for e in &mut out {
                e.score = 0.0;
                e.score_components = Default::default();
                e.rank_reasons.clear();
            }
            out
        };
        assert_eq!(
            run("[interests]\nalgebra = 1.0\ngeometry = 0.0"),
            run("[interests]\nalgebra = 0.0\ngeometry = 1.0")
        );
    }

    #[test]
    fn equal_ids_have_stable_source_ties() {
        let a = event(
            "same",
            "Talk A",
            Some("https://x.com/e"),
            None,
            vec![src("a", "https://x.com/e", Some("1"))],
        );
        let b = event(
            "same",
            "Talk B",
            Some("https://x.com/e"),
            None,
            vec![src("b", "https://x.com/e", Some("2"))],
        );
        assert_eq!(
            dedup_events(vec![a.clone(), b.clone()]),
            dedup_events(vec![b, a])
        );
    }

    #[test]
    fn indexed_matches_linear_reference_on_generated_corpora() {
        let mut seed = 0x5eed_cafe_1234_5678u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed >> 32
        };
        for case in 0..1200 {
            let size = (next() % 65) as usize;
            let mut events = Vec::new();
            for _ in 0..size {
                let id = format!("{:04}", next() % 24);
                let title = format!("Talk {}", next() % 8);
                let url = (next() % 4 != 0).then(|| format!("https://events.org/{}", next() % 20));
                let day = (next() % 3 != 0).then(|| date(2026, 9, (next() % 4 + 1) as u32));
                let sources = (0..next() % 4)
                    .map(|_| {
                        src(
                            &format!("s{}", next() % 4),
                            &format!("https://host{}.edu/{}", next() % 4, next() % 6),
                            (next() % 3 != 0)
                                .then(|| format!("{}", next() % 8))
                                .as_deref(),
                        )
                    })
                    .collect();
                let mut e = event(&id, &title, url.as_deref(), day, sources);
                if next() % 3 != 0 {
                    e.location = Some(Location {
                        name: format!("Venue {}", next() % 3),
                        city: None,
                        country: None,
                        venue: None,
                    });
                }
                if next() % 3 == 0 {
                    e.people.push(PersonHit {
                        canonical_name: format!("Org {}", next() % 3),
                        matched_text: "organizer".into(),
                        role: PersonRole::Organizer,
                        evidence: None,
                        confidence: 1.0,
                        scholar_tags: vec![],
                    });
                }
                if next() % 2 == 0 {
                    e.description = Some(format!("Description {}", next() % 3));
                }
                e.score = (next() % 100) as f32;
                events.push(e);
            }
            assert_eq!(
                dedup_events(events.clone()),
                reference_dedup_events(events),
                "seeded case {case}"
            );
        }
    }

    #[test]
    fn indexed_chooses_earliest_cluster_and_drops_obsolete_keys() {
        let mut a = event(
            "a",
            "Talk",
            Some("https://a.org/e"),
            Some(date(2026, 9, 1)),
            vec![src("a", "https://a.org/e", None)],
        );
        let b = event(
            "b",
            "Other",
            Some("https://b.org/e"),
            None,
            vec![src("b", "https://b.org/e", Some("native"))],
        );
        let mut bridge = a.clone();
        bridge.id = EventId("c".into());
        bridge.sources = b.sources.clone();
        bridge.people.push(PersonHit {
            canonical_name: "New organizer".into(),
            matched_text: "".into(),
            role: PersonRole::Organizer,
            evidence: None,
            confidence: 1.0,
            scholar_tags: vec![],
        });
        // This event matches A's old domain organizer but not the new explicit one.
        a.id = EventId("d".into());
        a.url = Some(Url::parse("https://a.org/different").unwrap());
        let inputs = vec![
            event(
                "a",
                "Talk",
                Some("https://a.org/e"),
                Some(date(2026, 9, 1)),
                vec![src("a", "https://a.org/e", None)],
            ),
            b,
            bridge,
            a,
        ];
        let result = dedup_events(inputs.clone());
        assert_eq!(result, reference_dedup_events(inputs));
        assert_eq!(
            result.len(),
            3,
            "bridge must not fuse existing clusters or retain an obsolete organizer key"
        );
        assert_eq!(result[0].sources.len(), 2);
    }

    #[test]
    fn index_removal_reveals_next_cluster_and_distinct_work_is_linear() {
        let key = IdentityKey::Url("https://example.org/e".into());
        let mut index = DedupIndex::default();
        index.insert(5, std::slice::from_ref(&key));
        index.insert(2, std::slice::from_ref(&key));
        assert_eq!(index.first_match(std::slice::from_ref(&key)), Some(2));
        index.remove(2, std::slice::from_ref(&key));
        assert_eq!(index.first_match(&[key]), Some(5));
        for n in [1000, 5000, 10000] {
            let inputs = (0..n)
                .map(|i| {
                    event(
                        &format!("{i:05}"),
                        "Talk",
                        Some(&format!("https://example.org/{i}")),
                        None,
                        vec![],
                    )
                })
                .collect();
            let mut index = DedupIndex::default();
            assert_eq!(dedup_indexed(inputs, false, &mut index).0.len(), n);
            assert_eq!(index.lookups, n, "one URL key lookup per distinct event");
        }
    }

    #[test]
    fn growing_provenance_has_bounded_index_work() {
        let n = 10_000;
        let inputs = (0..n)
            .map(|i| {
                event(
                    &format!("{i:05}"),
                    "Talk",
                    Some("https://example.org/e"),
                    None,
                    vec![src(
                        &format!("s{i}"),
                        &format!("https://example.org/{i}"),
                        Some(&i.to_string()),
                    )],
                )
            })
            .collect();
        let mut index = DedupIndex::default();
        let output = dedup_indexed(inputs, false, &mut index).0;
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].sources.len(), n);
        assert_eq!(index.lookups, 2 * n);
        assert!(
            index.updates <= 3 * n,
            "must not reindex the entire growing representative: {}",
            index.updates
        );
    }

    #[test]
    fn aliases_point_to_final_representatives() {
        let a = event("a", "Talk", Some("https://a.org/e"), None, vec![]);
        let b = event("b", "Talk", Some("https://a.org/e"), None, vec![]);
        let (events, aliases) = dedup_events_with_aliases(vec![b, a]);
        assert_eq!(events.len(), 1);
        assert_eq!(aliases.get(&EventId("b".into())), Some(&events[0].id));
        assert!(!aliases.contains_key(&events[0].id));
    }

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn canonicalize_url_strips_fragment_query_and_case() {
        let a = canonicalize_url(&Url::parse("https://Example.com/events/42").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/events/42#top").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_url_strips_trailing_slash_and_default_port() {
        let a = canonicalize_url(&Url::parse("https://host.com:443/path/").unwrap());
        let b = canonicalize_url(&Url::parse("https://host.com/path").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_url_strips_www_prefix() {
        let a = canonicalize_url(&Url::parse("https://www.example.com/e1").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e1").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_url_preserves_meaningful_params() {
        let a = canonicalize_url(&Url::parse("https://example.com/e?session=abc").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e?session=xyz").unwrap());
        assert_ne!(a, b, "distinct session params must not canonicalize equal");
        let c = canonicalize_url(&Url::parse("https://example.com/e?session=abc").unwrap());
        assert_eq!(a, c, "same session param must canonicalize equal");
    }

    #[test]
    fn canonicalize_url_drops_tracking_params() {
        let a = canonicalize_url(
            &Url::parse("https://example.com/e?utm_source=nl&fbclid=xyz").unwrap(),
        );
        let b = canonicalize_url(&Url::parse("https://example.com/e").unwrap());
        assert_eq!(a, b, "tracking params must be dropped");
    }

    // Generic keys `ref`, `source`, `ver` are NOT tracking params — they may
    // distinguish distinct calendar references, so they must be preserved to
    // avoid over-merging (§47).
    #[test]
    fn canonicalize_url_preserves_generic_ref_source_ver() {
        let a = canonicalize_url(&Url::parse("https://example.com/e?ref=calendar_a").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e?ref=calendar_b").unwrap());
        assert_ne!(a, b, "distinct ref values must not canonicalize equal");

        let c = canonicalize_url(&Url::parse("https://example.com/e?source=feed_a").unwrap());
        let d = canonicalize_url(&Url::parse("https://example.com/e?source=feed_b").unwrap());
        assert_ne!(c, d, "distinct source values must not canonicalize equal");

        let e = canonicalize_url(&Url::parse("https://example.com/e?ver=1").unwrap());
        let f = canonicalize_url(&Url::parse("https://example.com/e?ver=2").unwrap());
        assert_ne!(e, f, "distinct ver values must not canonicalize equal");

        // A tracking param alongside a generic ref: tracking dropped, ref kept.
        let g = canonicalize_url(
            &Url::parse("https://example.com/e?utm_source=nl&ref=calendar_a").unwrap(),
        );
        let h = canonicalize_url(&Url::parse("https://example.com/e?ref=calendar_a").unwrap());
        assert_eq!(g, h, "utm_source dropped, ref preserved");
    }

    #[test]
    fn canonicalize_url_sorts_query_params() {
        let a = canonicalize_url(&Url::parse("https://example.com/e?b=2&a=1").unwrap());
        let b = canonicalize_url(&Url::parse("https://example.com/e?a=1&b=2").unwrap());
        assert_eq!(
            a, b,
            "params must be sorted for deterministic canonicalization"
        );
    }

    #[test]
    fn canonicalize_url_preserves_tracking_alongside_meaningful() {
        let a = canonicalize_url(
            &Url::parse("https://example.com/e?utm_source=nl&session=42").unwrap(),
        );
        let b = canonicalize_url(&Url::parse("https://example.com/e?session=42").unwrap());
        assert_eq!(a, b, "tracking dropped, meaningful preserved");
    }

    #[test]
    fn domain_key_handles_ac_uk() {
        let url = Url::parse("https://www.maths.ox.ac.uk/events").unwrap();
        assert_eq!(domain_key(&url).unwrap(), "ox.ac.uk");
    }

    #[test]
    fn domain_key_handles_co_jp() {
        let url = Url::parse("https://example.co.jp/page").unwrap();
        assert_eq!(domain_key(&url).unwrap(), "example.co.jp");
    }

    #[test]
    fn domain_key_plain_two_label_tld() {
        let url = Url::parse("https://www.claymath.org/feed").unwrap();
        assert_eq!(domain_key(&url).unwrap(), "claymath.org");
    }

    #[test]
    fn canonical_url_match_merges() {
        let a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        assert!(are_duplicates(&a, &b, DedupSignal::CanonicalUrl));
        assert!(duplicate_signal(&a, &b) == Some(DedupSignal::CanonicalUrl));
    }

    #[test]
    fn different_urls_do_not_match_on_canonical_url() {
        let a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk",
            Some("https://x.com/e2"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::CanonicalUrl));
    }

    #[test]
    fn source_canonical_id_match_merges_across_different_urls() {
        let a = event(
            "a",
            "Indico Talk",
            Some("https://indico.com/event/1"),
            Some(date(2026, 8, 9)),
            vec![src("indico", "https://indico.com/api", Some("1"))],
        );
        let b = event(
            "b",
            "Indico Talk",
            Some("https://mirror.com/event/1"),
            Some(date(2026, 8, 9)),
            vec![src("indico", "https://mirror.com/api", Some("1"))],
        );
        assert!(are_duplicates(&a, &b, DedupSignal::SourceCanonicalId));
        assert!(duplicate_signal(&a, &b) == Some(DedupSignal::SourceCanonicalId));
    }

    #[test]
    fn source_canonical_id_requires_matching_source_id() {
        let a = event(
            "a",
            "Talk",
            None,
            Some(date(2026, 8, 9)),
            vec![src("indico", "https://indico.com", Some("1"))],
        );
        let b = event(
            "b",
            "Talk",
            None,
            Some(date(2026, 8, 9)),
            vec![src("other", "https://other.com", Some("1"))],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::SourceCanonicalId));
    }

    #[test]
    fn same_title_date_organizer_domain_merges() {
        let a = event(
            "a",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://math.mit.edu/events", None)],
        );
        let b = event(
            "b",
            "algebraic geometry conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://math.mit.edu/calendar", None)],
        );
        assert!(are_duplicates(&a, &b, DedupSignal::TitleDateOrganizer));
    }

    #[test]
    fn same_title_different_date_does_not_merge_on_organizer() {
        let a = event(
            "a",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://math.mit.edu/events", None)],
        );
        let b = event(
            "b",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 9, 9)),
            vec![src("s2", "https://math.mit.edu/calendar", None)],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::TitleDateOrganizer));
        assert!(duplicate_signal(&a, &b).is_none());
    }

    #[test]
    fn different_title_does_not_merge() {
        let a = event(
            "a",
            "Algebraic Geometry Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://math.mit.edu/events", None)],
        );
        let b = event(
            "b",
            "Number Theory Conference",
            None,
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://math.mit.edu/calendar", None)],
        );
        assert!(!are_duplicates(&a, &b, DedupSignal::TitleDateOrganizer));
        assert!(duplicate_signal(&a, &b).is_none());
    }

    #[test]
    fn dedup_events_merges_a_pair() {
        let a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        let out = dedup_events(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sources.len(), 2);
    }

    #[test]
    fn dedup_events_keeps_distinct_events_separate() {
        let a = event(
            "a",
            "Talk A",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        let b = event(
            "b",
            "Talk B",
            Some("https://x.com/e2"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        let out = dedup_events(vec![a, b]);
        assert_eq!(out.len(), 2);
    }

    // CORE-18: merge_events fills scalar gaps from the secondary event.
    #[test]
    fn merge_events_fills_scalar_gaps() {
        let mut a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        a.score = 10.0;
        a.description = Some("Description from A".into());
        a.location = Some(Location {
            name: "MIT".into(),
            city: Some("Cambridge".into()),
            country: None,
            venue: None,
        });

        let mut b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        b.score = 5.0;
        b.description = None;
        b.location = None;

        let merged = merge_events(a, b);
        assert_eq!(merged.description.as_deref(), Some("Description from A"));
        assert!(
            merged.location.is_some(),
            "location preserved from canonical primary"
        );
    }

    #[test]
    fn merge_events_fills_from_secondary_when_primary_lacks() {
        let mut a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        a.score = 10.0;
        a.description = None;
        a.location = None;

        let mut b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        b.score = 5.0;
        b.description = Some("Description from B".into());
        b.location = Some(Location {
            name: "MIT".into(),
            city: None,
            country: None,
            venue: None,
        });

        let merged = merge_events(a, b);
        assert_eq!(
            merged.description.as_deref(),
            Some("Description from B"),
            "description filled from secondary when primary lacks it"
        );
        assert!(
            merged.location.is_some(),
            "location filled from secondary when primary lacks it"
        );
    }

    // R9-H07: union_talks must merge same-ID talks, not drop the secondary.
    // Before the fix, a talk present in both events (same TalkId) lost the
    // secondary's speakers/media entirely — if the primary lacked speakers
    // but the secondary had them, the merged event had a speakerless talk.
    #[test]
    fn merge_events_unions_same_id_talk_speakers() {
        let talk_src_a = src("s1", "https://x.com/feed", None);
        let talk_src_b = src("s2", "https://y.com/feed", None);
        let talk_a = Talk {
            id: TalkId("t1".into()),
            title: "Algebraic Geometry".into(),
            speaker: Vec::new(),
            date_time: None,
            abstract_text: None,
            topics: Vec::new(),
            media: Vec::new(),
            source: talk_src_a,
        };
        let talk_b = Talk {
            id: TalkId("t1".into()),
            title: "Algebraic Geometry".into(),
            speaker: vec![PersonHit {
                canonical_name: "Alice".into(),
                matched_text: "Alice".into(),
                role: PersonRole::Speaker,
                evidence: None,
                confidence: 1.0,
                scholar_tags: Vec::new(),
            }],
            date_time: None,
            abstract_text: Some("Abstract from B".into()),
            topics: Vec::new(),
            media: Vec::new(),
            source: talk_src_b,
        };

        let mut a = event(
            "a",
            "Talk",
            Some("https://x.com/e1"),
            Some(date(2026, 8, 9)),
            vec![src("s1", "https://x.com/feed", None)],
        );
        a.score = 10.0;
        a.talks = vec![talk_a];

        let mut b = event(
            "b",
            "Talk",
            Some("https://x.com/e1#sec"),
            Some(date(2026, 8, 9)),
            vec![src("s2", "https://y.com/feed", None)],
        );
        b.score = 5.0;
        b.talks = vec![talk_b];

        let merged = merge_events(a, b);
        assert_eq!(merged.talks.len(), 1, "same-ID talks must merge into one");
        let t = &merged.talks[0];
        assert_eq!(t.speaker.len(), 1, "secondary speaker must be preserved");
        assert_eq!(t.speaker[0].canonical_name, "Alice");
        assert_eq!(
            t.abstract_text.as_deref(),
            Some("Abstract from B"),
            "secondary abstract fills gap in primary"
        );
    }
}
