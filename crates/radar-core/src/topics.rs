//! Topic model (§7). MVP uses canonical topic + aliases + phrases, no semantic
//! model. User interest weights alter ranking only; they never delete events.
use crate::normalize::{normalize_name, word_boundaries};
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

/// Match `text` against `topics`, returning one [`TopicMatch`] per matched
/// topic (§7, §6.2).
///
/// Each topic contributes at most one match: the canonical name (confidence
/// `1.0`) beats any alias match (confidence `0.8`); on a tie the first
/// candidate in declaration order wins. Single-word candidates use
/// word-boundary matching so "analysis" does not match inside
/// "psychoanalysis"; multi-word phrases use substring matching on the
/// normalized text.
pub fn match_topics(text: &str, topics: &[TopicRecord]) -> Vec<TopicMatch> {
    let norm_text = normalize_name(text);
    let text_words = word_boundaries(&norm_text);

    let mut matches: Vec<TopicMatch> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for topic in topics {
        // Candidates: (original surface form, is_canonical). Canonical name is
        // first so a canonical match beats alias matches; aliases follow in
        // declared order so the first alias wins on a tie.
        let mut candidates: Vec<(&str, bool)> = vec![(&topic.name, true)];
        for alias in &topic.aliases {
            candidates.push((alias, false));
        }

        // Select the best matching candidate. Replace only when a canonical
        // match supersedes a prior alias match; otherwise the first match wins.
        let mut best: Option<(&str, bool)> = None;
        for &(candidate, is_canonical) in &candidates {
            let norm_candidate = normalize_name(candidate);
            if norm_candidate.is_empty() {
                continue;
            }
            let is_match = if norm_candidate.contains(char::is_whitespace) {
                // Multi-word phrase: substring match (distinctive enough per §6.2).
                norm_text.contains(norm_candidate.as_str())
            } else {
                // Single token: word-boundary match so "analysis" does not match
                // inside "psychoanalysis".
                text_words.iter().any(|w| w.as_str() == norm_candidate)
            };
            if is_match {
                let take = match best {
                    None => true,
                    Some((_, prev_canonical)) => is_canonical && !prev_canonical,
                };
                if take {
                    best = Some((candidate, is_canonical));
                }
            }
        }

        if let Some((original, is_canonical)) = best {
            let confidence = if is_canonical { 1.0 } else { 0.8 };
            let m = TopicMatch {
                topic_id: topic.id.clone(),
                canonical_name: topic.name.clone(),
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
