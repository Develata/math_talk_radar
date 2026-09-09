//! Registry enrichment of already-parsed events. No interest weights or I/O.
use radar_core::Event;
use radar_core::normalize::normalize_name;
use radar_core::people::{MatchContext, ScholarRecord};
use radar_core::topics::{NormalizedTopic, match_topics_normalized};

/// CORE-11: match the event's title and description against the curated topic
/// registry and populate `event.topics`. Adapters set `topics: Vec::new()`;
/// without this step the 30-point topic component is always zero.
///
/// The title is matched first (canonical names are more likely there), then
/// the description; matches are deduplicated by `topic_id`, keeping the
/// highest-confidence hit so a canonical title match beats an alias match from
/// the description.
pub(super) fn enrich_event_topics(event: &mut Event, topics: &[NormalizedTopic]) {
    if topics.is_empty() {
        return;
    }
    // CLI-22: merge registry matches with adapter-set topics instead of
    // replacing. The old code did `event.topics = matches`, which wiped any
    // topics an adapter had already populated when the registry found no
    // matches (or found different topic_ids). Keep adapter topics, then add
    // registry matches, deduping by topic_id and keeping the higher
    // confidence hit.
    let mut matches = match_topics_normalized(&event.title, topics);
    if let Some(desc) = event.description.as_ref()
        && !desc.is_empty()
    {
        let desc_matches = match_topics_normalized(desc, topics);
        for m in desc_matches {
            if let Some(existing) = matches.iter_mut().find(|e| e.topic_id == m.topic_id) {
                if m.confidence > existing.confidence {
                    *existing = m;
                }
            } else {
                matches.push(m);
            }
        }
    }
    // Merge: start from adapter topics, then fold in registry matches.
    let mut merged = std::mem::take(&mut event.topics);
    for m in matches {
        if let Some(existing) = merged.iter_mut().find(|e| e.topic_id == m.topic_id) {
            if m.confidence > existing.confidence {
                *existing = m;
            }
        } else {
            merged.push(m);
        }
    }
    event.topics = merged;
}

/// CORE-12: enrich the event's people list with scholar tags from the curated
/// registry and add title-mentioned scholars that adapters did not surface.
/// Adapters construct `PersonHit`s from parsed speaker fields but set
/// `scholar_tags: Vec::new()`; without this step the people component can
/// never exceed the 3-point baseline for a non-TitleMention role, so
/// important laureates (Fields/Abel/Wolf/Crafoord) are never recognized.
///
/// Two passes:
/// 1. For each adapter-found person, look up the scholar by normalized
///    canonical name. On a hit, back-fill `scholar_tags` and correct the
///    `canonical_name` to the registry's canonical form (adapters may use a
///    variant surface form).
/// 2. Run `match_scholars` on the title under `TitleText` context to find
///    important scholars mentioned only in the title. Add any not already
///    present (by normalized canonical name) as `TitleMention` so the people
///    component can still recognize them for ranking.
pub(super) fn enrich_event_scholars(event: &mut Event, scholars: &[ScholarRecord]) {
    if scholars.is_empty() {
        return;
    }

    // Pass 1: back-fill scholar_tags on adapter-found people.
    for person in &mut event.people {
        if !person.scholar_tags.is_empty() {
            continue;
        }
        let norm_name = normalize_name(&person.canonical_name);
        if let Some(scholar) = scholars
            .iter()
            .find(|s| normalize_name(&s.canonical_name) == norm_name)
        {
            person.scholar_tags = scholar.tags.clone();
            person.canonical_name = scholar.canonical_name.clone();
        }
    }

    // Pass 2: add title-mentioned scholars not already present.
    let title_hits =
        radar_core::people::match_scholars(&event.title, scholars, MatchContext::TitleText);
    for hit in title_hits {
        let norm_canonical = normalize_name(&hit.canonical_name);
        let already_present = event
            .people
            .iter()
            .any(|p| normalize_name(&p.canonical_name) == norm_canonical);
        if !already_present {
            event.people.push(hit);
        }
    }
}
