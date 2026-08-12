//! Topic model (§7). MVP uses canonical topic + aliases + phrases, no semantic
//! model. User interest weights alter ranking only; they never delete events.
use crate::normalize::{contains_phrase, normalize_name};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopicMatch {
    pub topic_id: String,
    pub canonical_name: String,
    pub matched_text: String,
    pub confidence: f32,
}

/// A topic entry loaded from `config/topics.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Pre-normalized topic representation: canonical name and every candidate
/// (canonical + aliases) already run through [`normalize_name`]. Built once
/// per scan via [`normalize_topics`] and reused across every event by
/// [`match_topics_normalized`], so the per-event hot path skips the
/// normalization it would otherwise repeat for each candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedTopic {
    pub id: String,
    pub canonical_name: String,
    /// (original surface form, normalized form, is_canonical). Canonical name
    /// is first so a canonical match beats alias matches; aliases follow in
    /// declared order so the first alias wins on a tie.
    candidates: Vec<(String, String, bool)>,
}

/// Pre-normalize `topics` once per scan so the per-event matching path no
/// longer re-normalizes each candidate. The returned [`Vec<NormalizedTopic>`]
/// is fed to [`match_topics_normalized`].
pub fn normalize_topics(topics: &[TopicRecord]) -> Vec<NormalizedTopic> {
    topics
        .iter()
        .map(|topic| {
            let mut candidates = Vec::with_capacity(topic.aliases.len() + 1);
            candidates.push((topic.name.clone(), normalize_name(&topic.name), true));
            for alias in &topic.aliases {
                candidates.push((alias.clone(), normalize_name(alias), false));
            }
            NormalizedTopic {
                id: topic.id.clone(),
                canonical_name: topic.name.clone(),
                candidates,
            }
        })
        .collect()
}

/// Match `text` against pre-normalized `topics`, returning one [`TopicMatch`]
/// per matched topic (§7, §6.2). This is the hot path used per event during a
/// scan; callers build `topics` once via [`normalize_topics`].
///
/// Each topic contributes at most one match: the canonical name (confidence
/// `1.0`) beats any alias match (confidence `0.8`); on a tie the first
/// candidate in declaration order wins. Single-word candidates use
/// word-boundary matching so "analysis" does not match inside
/// "psychoanalysis"; multi-word phrases use substring matching on the
/// normalized text.
pub fn match_topics_normalized(text: &str, topics: &[NormalizedTopic]) -> Vec<TopicMatch> {
    let norm_text = normalize_name(text);

    let mut matches: Vec<TopicMatch> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for topic in topics {
        // Select the best matching candidate. Replace only when a canonical
        // match supersedes a prior alias match; otherwise the first match wins.
        let mut best: Option<(&str, bool)> = None;
        for (surface, normalized, is_canonical) in &topic.candidates {
            if normalized.is_empty() {
                continue;
            }
            // `contains_phrase` handles both single and multi-word candidates
            // with word-boundary awareness, so "analysis" does not match inside
            // "psychoanalysis" and "pde" matches "(pde)" or "pde,".
            let is_match = contains_phrase(&norm_text, normalized);
            if is_match {
                let take = match best {
                    None => true,
                    Some((_, prev_canonical)) => *is_canonical && !prev_canonical,
                };
                if take {
                    best = Some((surface, *is_canonical));
                }
            }
        }

        if let Some((original, is_canonical)) = best {
            let confidence = if is_canonical { 1.0 } else { 0.8 };
            let m = TopicMatch {
                topic_id: topic.id.clone(),
                canonical_name: topic.canonical_name.clone(),
                matched_text: original.to_string(),
                confidence,
            };
            if let Some(&idx) = seen.get(&topic.id) {
                if confidence > matches[idx].confidence {
                    matches[idx] = m;
                }
            } else {
                seen.insert(topic.id.clone(), matches.len());
                matches.push(m);
            }
        }
    }

    matches
}

/// Match `text` against `topics`, returning one [`TopicMatch`] per matched
/// topic (§7, §6.2). Convenience wrapper that pre-normalizes `topics` on each
/// call; for scan hot paths prefer building a [`NormalizedTopic`] list once
/// via [`normalize_topics`] and calling [`match_topics_normalized`] per event.
pub fn match_topics(text: &str, topics: &[TopicRecord]) -> Vec<TopicMatch> {
    let normalized = normalize_topics(topics);
    match_topics_normalized(text, &normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arithmetic_geometry() -> TopicRecord {
        TopicRecord {
            id: "arithmetic_geometry".into(),
            name: "Arithmetic Geometry".into(),
            aliases: vec![
                "arithmetic geometry".into(),
                "Shimura varieties".into(),
                "Diophantine geometry".into(),
            ],
        }
    }

    fn algebraic_geometry() -> TopicRecord {
        TopicRecord {
            id: "algebraic_geometry".into(),
            name: "Algebraic Geometry".into(),
            aliases: vec![
                "algebraic geometry".into(),
                "schemes".into(),
                "derived algebraic geometry".into(),
            ],
        }
    }

    fn analysis() -> TopicRecord {
        TopicRecord {
            id: "analysis".into(),
            name: "Analysis".into(),
            aliases: vec![
                "analysis".into(),
                "harmonic analysis".into(),
                "PDE".into(),
                "partial differential equations".into(),
            ],
        }
    }

    // TOP-001: topic alias matching.
    #[test]
    fn top_001_alias_match_via_shimura_varieties() {
        let ag = arithmetic_geometry();
        let result = match_topics("Workshop on Shimura varieties", std::slice::from_ref(&ag));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].topic_id, "arithmetic_geometry");
        assert_eq!(result[0].canonical_name, "Arithmetic Geometry");
        assert_eq!(result[0].matched_text, "Shimura varieties");
        assert!((result[0].confidence - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn top_001_canonical_match() {
        let ag = arithmetic_geometry();
        let result = match_topics("Arithmetic Geometry conference", std::slice::from_ref(&ag));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].topic_id, "arithmetic_geometry");
        assert_eq!(result[0].matched_text, "Arithmetic Geometry");
        assert!((result[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    // Supplementary: single-word alias uses word-boundary matching.
    #[test]
    fn single_word_alias_word_boundary_match() {
        let a = analysis();
        let result = match_topics("a course on analysis", std::slice::from_ref(&a));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].topic_id, "analysis");
    }

    #[test]
    fn single_word_alias_word_boundary_prevents_partial_match() {
        let a = analysis();
        let result = match_topics("psychoanalysis is not math", std::slice::from_ref(&a));
        assert!(result.is_empty());
    }

    // T2-1: single-token alias must match when adjacent to punctuation.
    // `unicode_words` strips punctuation, so the old `w == candidate` check
    // failed for "pde" inside "(pde)" or "pde,". `contains_phrase` is
    // boundary-aware and handles these cases.
    #[test]
    fn single_word_alias_matches_adjacent_to_punctuation() {
        let a = analysis();
        for text in [
            "a course on (pde)",
            "pde, and applications",
            "topics: pde.",
            "nonlinear pde; recent progress",
        ] {
            let result = match_topics(text, std::slice::from_ref(&a));
            assert_eq!(
                result.len(),
                1,
                "expected single-token alias 'pde' to match in {text:?}, got {result:?}"
            );
            assert_eq!(result[0].topic_id, "analysis");
        }
    }

    #[test]
    fn single_word_alias_still_rejects_substring_inside_word() {
        let a = analysis();
        let result = match_topics("the pdepde conference", std::slice::from_ref(&a));
        assert!(
            result.is_empty(),
            "'pde' must not match inside 'pdepde' (no word boundary)"
        );
    }

    // Supplementary: multiple topics match independently.
    #[test]
    fn multiple_topics_match() {
        let ag = arithmetic_geometry();
        let alg = algebraic_geometry();
        let result = match_topics("arithmetic geometry and algebraic geometry", &[ag, alg]);
        assert_eq!(result.len(), 2);
        assert!(
            result.iter().any(|m| m.topic_id == "arithmetic_geometry"),
            "expected arithmetic_geometry match, got {:?}",
            result
        );
        assert!(
            result.iter().any(|m| m.topic_id == "algebraic_geometry"),
            "expected algebraic_geometry match, got {:?}",
            result
        );
    }

    // Supplementary: no match.
    #[test]
    fn no_match_returns_empty() {
        let ag = arithmetic_geometry();
        let a = analysis();
        let result = match_topics("no math topics here", &[ag, a]);
        assert!(result.is_empty());
    }

    // Supplementary: deduplication keeps the canonical (highest confidence).
    #[test]
    fn dedup_keeps_canonical_confidence() {
        let ag = arithmetic_geometry();
        let result = match_topics(
            "arithmetic geometry and Shimura varieties",
            std::slice::from_ref(&ag),
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].topic_id, "arithmetic_geometry");
        assert!((result[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    // Supplementary: empty topics list.
    #[test]
    fn empty_topics_returns_empty() {
        let result = match_topics("anything", &[]);
        assert!(result.is_empty());
    }
}
