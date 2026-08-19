//! Person model and scholar matching (§6, §6.1, §6.2).
//!
//! Role protection (§P-2, §6.2): a name in body text can yield at most
//! `TitleMention` / `Unknown`. Structured person fields or strong
//! name-in-context evidence are required for `Speaker` / `Organizer` / etc.
use crate::model::Event;
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

/// Pre-normalized scholar representation: canonical name and every candidate
/// (canonical + aliases) already run through [`normalize_name`]. Built once
/// per scan via [`normalize_scholars`] and reused across every event by
/// [`match_scholars_normalized`] and [`enrich_event_scholars`], so the
/// per-event hot path skips the normalization it would otherwise repeat for
/// each candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedScholar {
    pub id: String,
    pub canonical_name: String,
    pub tags: Vec<String>,
    /// (original surface form, normalized form). Canonical name is first so
    /// ties prefer it; final selection uses max length for `matched_text`.
    candidates: Vec<(String, String)>,
}

impl NormalizedScholar {
    /// The normalized form of the canonical name, for exact-match lookup
    /// (used by [`enrich_event_scholars`] pass 1).
    pub fn normalized_canonical(&self) -> &str {
        &self.candidates[0].1
    }

    /// True if any candidate (canonical name or alias) normalizes to `name`.
    /// Used by [`enrich_event_scholars`] pass 1 to back-fill `scholar_tags`
    /// when an adapter surfaced a speaker under an alias surface form (e.g.
    /// "Zagier" instead of the canonical "Don Zagier") — without this the
    /// laureate ranking boost never attaches.
    pub fn matches_normalized(&self, name: &str) -> bool {
        self.candidates.iter().any(|(_, n)| n == name)
    }
}

/// Pre-normalize `scholars` once per scan so the per-event matching path no
/// longer re-normalizes each candidate. The returned [`Vec<NormalizedScholar>`]
/// is fed to [`match_scholars_normalized`] and [`enrich_event_scholars`].
pub fn normalize_scholars(scholars: &[ScholarRecord]) -> Vec<NormalizedScholar> {
    scholars
        .iter()
        .map(|s| {
            let mut candidates = Vec::with_capacity(s.aliases.len() + 1);
            candidates.push((s.canonical_name.clone(), normalize_name(&s.canonical_name)));
            for alias in &s.aliases {
                candidates.push((alias.clone(), normalize_name(alias)));
            }
            NormalizedScholar {
                id: s.id.clone(),
                canonical_name: s.canonical_name.clone(),
                tags: s.tags.clone(),
                candidates,
            }
        })
        .collect()
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

/// Match pre-normalized `scholars` against `text` under the given
/// [`MatchContext`] (§6.2). This is the hot path used per event during a
/// scan; callers build `scholars` once via [`normalize_scholars`].
///
/// Each scholar contributes at most one hit; when multiple candidates match,
/// the longest surface form is kept as `matched_text` (most distinctive form).
/// See [`match_scholars`] for the full semantics; this variant only skips
/// per-call normalization of the scholar candidates.
pub fn match_scholars_normalized(
    text: &str,
    scholars: &[NormalizedScholar],
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
        // Candidates are pre-normalized: (surface form, normalized form).
        let norm_canonical = &scholar.candidates[0].1;

        // Collect every candidate whose normalized form matches the text.
        let mut matched: Vec<&str> = Vec::new();
        for (original, normalized) in &scholar.candidates {
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

/// Match `scholars` against `text` under the given [`MatchContext`] (§6.2).
/// Convenience wrapper that pre-normalizes `scholars` on each call; for scan
/// hot paths prefer building a [`NormalizedScholar`] list once via
/// [`normalize_scholars`] and calling [`match_scholars_normalized`] per event.
pub fn match_scholars(
    text: &str,
    scholars: &[ScholarRecord],
    context: MatchContext,
) -> Vec<PersonHit> {
    let normalized = normalize_scholars(scholars);
    match_scholars_normalized(text, &normalized, context)
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
///    canonical name (using the pre-normalized [`NormalizedScholar`] list).
///    On a hit, back-fill `scholar_tags` and correct the `canonical_name` to
///    the registry's canonical form (adapters may use a variant surface form).
/// 2. Run [`match_scholars_normalized`] on the title under `TitleText` context
///    to find important scholars mentioned only in the title. Add any not
///    already present (by normalized canonical name) as `TitleMention` so the
///    people component can still recognize them for ranking.
pub fn enrich_event_scholars(event: &mut Event, scholars: &[NormalizedScholar]) {
    if scholars.is_empty() {
        return;
    }

    // Pass 1: back-fill scholar_tags on adapter-found people.
    // Match against any candidate (canonical name OR alias) so a speaker
    // surfaced under an alias form (e.g. "Zagier") still attaches the
    // laureate tags of the canonical scholar ("Don Zagier").
    for person in &mut event.people {
        if !person.scholar_tags.is_empty() {
            continue;
        }
        let norm_name = normalize_name(&person.canonical_name);
        if let Some(scholar) = scholars.iter().find(|s| s.matches_normalized(&norm_name)) {
            person.scholar_tags = scholar.tags.clone();
            person.canonical_name = scholar.canonical_name.clone();
        }
    }

    // Pass 2: add title-mentioned scholars not already present.
    let title_hits = match_scholars_normalized(&event.title, scholars, MatchContext::TitleText);
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

    // BUG-3: enrich_event_scholars pass 1 previously matched only the
    // canonical name, so a Speaker surfaced under an alias (e.g. "Zagier")
    // never received scholar_tags and the canonical_name was not corrected.
    // A laureate (wolf tag) recognized by alias would then lose the ranking
    // boost entirely.
    #[test]
    fn per_004_enrich_attaches_tags_for_alias_speaker() {
        let z = zagier();
        let normalized = normalize_scholars(std::slice::from_ref(&z));

        let mut event = Event {
            people: vec![PersonHit {
                canonical_name: "Zagier".into(),
                matched_text: "Zagier".into(),
                role: PersonRole::Speaker,
                evidence: None,
                confidence: 1.0,
                scholar_tags: Vec::new(),
            }],
            ..empty_event()
        };

        enrich_event_scholars(&mut event, &normalized);

        assert_eq!(event.people.len(), 1);
        let p = &event.people[0];
        assert_eq!(p.canonical_name, "Don Zagier");
        assert!(p.scholar_tags.contains(&"wolf".to_string()));
        assert!(p.scholar_tags.contains(&"curated".to_string()));
    }

    #[test]
    fn per_004_enrich_attaches_tags_for_canonical_speaker() {
        let z = zagier();
        let normalized = normalize_scholars(std::slice::from_ref(&z));

        let mut event = Event {
            people: vec![PersonHit {
                canonical_name: "Don Zagier".into(),
                matched_text: "Don Zagier".into(),
                role: PersonRole::Speaker,
                evidence: None,
                confidence: 1.0,
                scholar_tags: Vec::new(),
            }],
            ..empty_event()
        };

        enrich_event_scholars(&mut event, &normalized);

        assert_eq!(event.people.len(), 1);
        let p = &event.people[0];
        assert_eq!(p.canonical_name, "Don Zagier");
        assert!(p.scholar_tags.contains(&"wolf".to_string()));
    }

    fn empty_event() -> Event {
        use crate::model::{AccessInfo, EventStatus, EventType, OnlineAvailability, PublicAccess};
        Event {
            id: crate::model::EventId(String::new()),
            title: String::new(),
            url: None,
            event_type: EventType::Unknown,
            status: EventStatus::Unknown,
            date: crate::date::EventDate {
                start: None,
                end: None,
                timezone: None,
                original_text: String::new(),
                precision: crate::date::DatePrecision::Unknown,
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
            sources: Vec::new(),
            score: 0.0,
            score_components: crate::ranking::ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        }
    }
}
