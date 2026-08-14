//! Person model and scholar matching (§6, §6.1, §6.2).
//!
//! Role protection (§P-2, §6.2): a name in body text can yield at most
//! `TitleMention` / `Unknown`. Structured person fields or strong
//! name-in-context evidence are required for `Speaker` / `Organizer` / etc.
use crate::normalize::{contains_phrase, normalize_name, word_boundaries};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersonRole {
    Speaker,
    Lecturer,
    Organizer,
    Participant,
    Panelist,
    Honoree,
    SeriesNamesake,
    TitleMention,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonHit {
    pub canonical_name: String,
    pub matched_text: String,
    pub role: PersonRole,
    #[serde(default)]
    pub evidence: Option<String>,
    pub confidence: f32,
    #[serde(default)]
    pub scholar_tags: Vec<String>,
}

/// A curated scholar entry loaded from `config/scholars.toml` (§6.1).
/// Kept decoupled from any parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScholarRecord {
    pub id: String,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Wrapper for the `scholars.toml` document shape: a top-level
/// `[[scholars]]` array. Loaded by the CLI at startup from the embedded
/// default (§33, CFG-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScholarsConfig {
    #[serde(default)]
    pub scholars: Vec<ScholarRecord>,
}

impl ScholarsConfig {
    /// Parse a `scholars.toml` document. Returns a config with an empty
    /// scholar list for empty input.
    pub fn parse(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// The embedded default scholar registry shipped with the binary
    /// (CFG-001). B5: returns `Result` instead of panicking at runtime.
    pub fn embedded() -> Result<Self, toml::de::Error> {
        Self::parse(include_str!("../../../config/scholars.toml"))
    }
}

/// Controls role assignment per §6.2: a name in a structured person field can
/// yield a Speaker/Organizer/etc. role; a name in body text or a title yields at
/// most TitleMention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchContext {
    /// A structured person field (e.g. "Speaker: ...", organizer list). The
    /// enclosed role is assigned directly with confidence 1.0.
    StructuredField(PersonRole),
    /// Free-running body text (abstract, description). Yields at most
    /// TitleMention; short/ambiguous surnames require multi-token evidence.
    BodyText,
    /// Event/talk title text. Always yields TitleMention, never Speaker.
    TitleText,
}

/// Short surnames too common to trust as a sole body-text match (§6.2). In
/// [`MatchContext::BodyText`], a single-token alias that is one of these
/// requires ≥2 tokens of the canonical name to also appear in the text.
const AMBIGUOUS_SURNAMES: &[&str] = &["li", "wang", "tao", "yau", "gross", "wei", "wu"];

/// Match `scholars` against `text` under the given [`MatchContext`] (§6.2).
///
/// Pipeline: normalize text → for each scholar, normalize canonical name and
/// aliases → match single-token candidates with word-boundary semantics and
/// multi-word candidates as substrings → assign role per context, applying the
/// ambiguous-surname guard in [`MatchContext::BodyText`]. Returns one
/// [`PersonHit`] per matching scholar; when multiple candidates match, the
/// longest surface form is kept as `matched_text` (most distinctive form, e.g.
/// "Don B. Zagier" beats "Zagier").
pub fn match_scholars(
    text: &str,
    scholars: &[ScholarRecord],
    context: MatchContext,
) -> Vec<PersonHit> {
    let norm_text = normalize_name(text);
    // Only the BodyText ambiguous-surname guard needs word boundaries.
    let text_words = if context == MatchContext::BodyText {
        word_boundaries(&norm_text)
    } else {
        Vec::new()
    };

    let mut hits = Vec::new();
    for scholar in scholars {
        let norm_canonical = normalize_name(&scholar.canonical_name);

        // Candidates: (original surface form, normalized form). Canonical name
        // is first so ties prefer it; final selection below uses max length.
        let mut candidates: Vec<(&str, String)> =
            vec![(&scholar.canonical_name, norm_canonical.clone())];
        for alias in &scholar.aliases {
            candidates.push((alias, normalize_name(alias)));
        }

        // Collect every candidate whose normalized form matches the text.
        let mut matched: Vec<&str> = Vec::new();
        for (original, normalized) in &candidates {
            if normalized.is_empty() {
                continue;
            }
            let is_match = if normalized.contains(char::is_whitespace) {
                norm_text.contains(normalized.as_str())
            } else {
                contains_phrase(&norm_text, normalized)
            };
            if is_match {
                matched.push(original);
            }
        }

        if matched.is_empty() {
            continue;
        }

        // Deduplicate within a scholar: keep the longest matching surface form.
        let matched_text: &str = match matched.iter().max_by_key(|s| s.len()).copied() {
            Some(s) => s,
            None => continue,
        };
        let matched_norm = normalize_name(matched_text);
        let matched_is_single_word = !matched_norm.contains(char::is_whitespace);
        let matched_is_ambiguous =
            matched_is_single_word && AMBIGUOUS_SURNAMES.contains(&matched_norm.as_str());

        // Role assignment per context.
        let (role, confidence) = match context {
            MatchContext::StructuredField(r) => (r, 1.0),
            MatchContext::BodyText => {
                // Ambiguous-surname guard: if the match is a single ambiguous
                // surname, require ≥2 tokens of the canonical name to appear in
                // the text, so "Tao" alone in body text does not match but
                // "Terence Tao" does.
                if matched_is_ambiguous {
                    let tokens_in_text = norm_canonical
                        .split_whitespace()
                        .filter(|t| text_words.iter().any(|w| w.as_str() == *t))
                        .count();
                    if tokens_in_text < 2 {
                        continue;
                    }
                }
                (PersonRole::TitleMention, 0.5)
            }
            MatchContext::TitleText => (PersonRole::TitleMention, 0.5),
        };

        hits.push(PersonHit {
            canonical_name: scholar.canonical_name.clone(),
            matched_text: matched_text.to_string(),
            role,
            // `evidence` is reserved for a longer context span (e.g. the
            // sentence around the match). v0.1 does not extract context spans,
            // so it stays `None` rather than duplicating `matched_text`.
            evidence: None,
            confidence,
            scholar_tags: scholar.tags.clone(),
        });
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zagier() -> ScholarRecord {
        ScholarRecord {
            id: "don-zagier".into(),
            canonical_name: "Don Zagier".into(),
            aliases: vec!["Zagier".into(), "Don B. Zagier".into()],
            tags: vec!["wolf".into(), "curated".into()],
        }
    }

    fn tao() -> ScholarRecord {
        ScholarRecord {
            id: "terence-tao".into(),
            canonical_name: "Terence Tao".into(),
            aliases: vec!["Tao".into(), "Terry Tao".into(), "陶哲轩".into()],
            tags: vec!["fields".into()],
        }
    }

    fn deligne() -> ScholarRecord {
        ScholarRecord {
            id: "pierre-deligne".into(),
            canonical_name: "Pierre Deligne".into(),
            aliases: vec!["Deligne".into()],
            tags: vec![
                "fields".into(),
                "wolf".into(),
                "abel".into(),
                "crafoord".into(),
            ],
        }
    }

    fn gross() -> ScholarRecord {
        ScholarRecord {
            id: "benedict-gross".into(),
            canonical_name: "Benedict Gross".into(),
            aliases: vec!["Gross".into()],
            tags: vec![],
        }
    }

    // PER-001: scholar alias match via a structured person field.
    #[test]
    fn per_001_structured_field_canonical_and_alias() {
        let z = zagier();
        // Canonical name "Don Zagier" in a structured Speaker field.
        let hits = match_scholars(
            "Speaker: Don Zagier",
            std::slice::from_ref(&z),
            MatchContext::StructuredField(PersonRole::Speaker),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].canonical_name, "Don Zagier");
        assert_eq!(hits[0].role, PersonRole::Speaker);
        assert!((hits[0].confidence - 1.0).abs() < f32::EPSILON);

        // Alias "Zagier" alone in a structured Speaker field.
        let hits = match_scholars(
            "Speaker: Zagier",
            &[z],
            MatchContext::StructuredField(PersonRole::Speaker),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].canonical_name, "Don Zagier");
        assert_eq!(hits[0].role, PersonRole::Speaker);
        assert!((hits[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    // PER-002: multilingual (CJK) alias match via a structured person field.
    #[test]
    fn per_002_multilingual_alias_cjk() {
        let t = tao();
        let hits = match_scholars(
            "主讲：陶哲轩",
            &[t],
            MatchContext::StructuredField(PersonRole::Speaker),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].canonical_name, "Terence Tao");
        assert_eq!(hits[0].role, PersonRole::Speaker);
        assert!((hits[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    // PER-003: concept names must not be promoted to Speaker.
    #[test]
    fn per_003_concept_name_not_speaker_body_text() {
        let g = gross();
        let z = zagier();
        let hits = match_scholars("The Gross-Zagier formula", &[g, z], MatchContext::BodyText);
        // Body text yields at most TitleMention — never Speaker.
        assert!(
            !hits.iter().any(|h| h.role == PersonRole::Speaker),
            "body text must not promote to Speaker, got {:?}",
            hits
        );
        // Zagier (non-ambiguous surname) is allowed as TitleMention; Gross is
        // filtered out by the ambiguous-surname guard.
        assert!(
            hits.iter()
                .any(|h| h.canonical_name == "Don Zagier" && h.role == PersonRole::TitleMention)
        );
    }

    #[test]
    fn per_003_concept_name_in_title_text() {
        let d = deligne();
        let hits = match_scholars(
            "Deligne periods of modular forms",
            &[d],
            MatchContext::TitleText,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].role, PersonRole::TitleMention);
        assert!((hits[0].confidence - 0.5).abs() < f32::EPSILON);
        assert_ne!(hits[0].role, PersonRole::Speaker);
    }

    #[test]
    fn per_003_empty_scholars_smoke() {
        let hits = match_scholars("Ahlfors Lecture Series", &[], MatchContext::TitleText);
        assert!(hits.is_empty());
    }

    #[test]
    fn per_003_ambiguous_surname_alone_in_body_text() {
        let t = tao();
        // "Tao" alone (ambiguous surname) in body text → no match.
        let hits = match_scholars("Tao proved a theorem", &[t], MatchContext::BodyText);
        assert!(
            hits.is_empty(),
            "ambiguous surname alone must not match in body text"
        );
    }

    #[test]
    fn per_003_full_name_in_body_text_matches_title_mention() {
        let t = tao();
        // "Terence Tao" (2 tokens) in body text → TitleMention, not Speaker.
        let hits = match_scholars("Terence Tao proved a theorem", &[t], MatchContext::BodyText);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].canonical_name, "Terence Tao");
        assert_eq!(hits[0].role, PersonRole::TitleMention);
        assert!((hits[0].confidence - 0.5).abs() < f32::EPSILON);
    }
}
