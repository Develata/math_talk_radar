//! Score composition (§26). Pure function of event evidence; no I/O.
//!
//! Default score is 0–100. Title-only scholar mentions (e.g. "Gross-Zagier
//! formula", "Deligne periods") must NOT receive the people component. The
//! composition function lands in M1/M4; this module fixes the component shape
//! and per-signal caps.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::SourceTier;
use crate::model::{Event, MediaType, OnlineAvailability, PublicAccess};
use crate::people::PersonRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScoreComponents {
    pub topic: u8,
    pub media: u8,
    pub access: u8,
    pub source_tier: u8,
    pub people: u8,
    pub completeness: u8,
}

// Per-signal caps from §26. Sum = 100.
pub const MAX_TOPIC: u8 = 30;
pub const MAX_MEDIA: u8 = 25;
pub const MAX_ACCESS: u8 = 15;
pub const MAX_SOURCE_TIER: u8 = 10;
pub const MAX_PEOPLE: u8 = 10;
pub const MAX_COMPLETENESS: u8 = 10;

/// User interest weights mapping topic_id → weight in [0.0, 1.0]. Loaded from
/// `config/interests.example.toml`. Alters ranking only; never deletes events
/// (§7). `#[serde(transparent)]` is REQUIRED for TOML deserialization (tuple
/// structs deserialize as sequences by default).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InterestWeights(pub HashMap<String, f64>);

impl InterestWeights {
    /// Returns the weight for `topic_id`, or 1.0 if absent (neutral — no boost,
    /// no penalty).
    pub fn weight(&self, topic_id: &str) -> f64 {
        let raw = self.0.get(topic_id).copied().unwrap_or(1.0);
        if raw.is_finite() {
            raw.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Parse interest weights from a TOML string in the format:
    /// ```toml
    /// [interests]
    /// arithmetic_geometry = 1.0
    /// number_theory = 0.8
    /// ```
    pub fn parse(toml_str: &str) -> Result<Self, toml::de::Error> {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(default)]
            interests: HashMap<String, f64>,
        }
        let w: Wrapper = toml::from_str(toml_str)?;
        Ok(InterestWeights(w.interests))
    }
}

/// Round half up to u8, clamped to [0, 255]. Per-signal caps already bound
/// values; the clamp is a safety net.
fn round_to_u8(x: f64) -> u8 {
    let v = (x + 0.5).floor();
    if v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v as u8
    }
}

/// Compute the ranking score for `event` (§26). Pure and deterministic: a
/// function of `event` evidence and the source-tier registry. Interest weights
/// boost topic contributions but never zero them out (§7).
///
/// Returns the total score (0–100), the per-signal [`ScoreComponents`], and
/// human-readable `rank_reasons` for explainability (§26.1).
pub fn score_event(
    event: &Event,
    source_tiers: &HashMap<String, SourceTier>,
    interests: Option<&InterestWeights>,
) -> (f32, ScoreComponents, Vec<String>) {
    let mut rank_reasons: Vec<String> = Vec::new();

    // 1. Topic component (cap MAX_TOPIC). Each matched topic contributes
    //    15 * (0.5 + 0.5 * w), rounded half up, where w is the interest weight
    //    (1.0 if absent). Minimum contribution (w=0) is 7.5 → 8.
    let mut topic_sum: u8 = 0;
    for topic in &event.topics {
        let w = match interests {
            Some(iw) => iw.weight(&topic.topic_id),
            None => 1.0,
        };
        let contribution = round_to_u8(15.0 * (0.5 + 0.5 * w));
        topic_sum = topic_sum.saturating_add(contribution);
        if contribution > 0 {
            rank_reasons.push(format!("matched_topic: {}", topic.topic_id));
        }
    }
    let topic = topic_sum.min(MAX_TOPIC);

    // 2. Media component (cap MAX_MEDIA). Max over all media resources:
    //    open video=25, reg-required video=18, open audio=15,
    //    slides/lecture_notes=10, else 0.
    let mut media_score: u8 = 0;
    for media in &event.media {
        let s = media_signal(&media.media_type, media.public_access);
        if s > media_score {
            media_score = s;
        }
    }
    let media = media_score.min(MAX_MEDIA);
    if media > 0 {
        rank_reasons.push("public_recording_available".to_string());
    }

    // 3. Access component (cap MAX_ACCESS). Live-access signals only;
    //    RecordingAvailable is captured by the media component.
    let access = access_signal(event.access.online).min(MAX_ACCESS);
    match event.access.online {
        OnlineAvailability::Livestream => rank_reasons.push("livestream".to_string()),
        OnlineAvailability::Hybrid => rank_reasons.push("hybrid".to_string()),
        _ => {}
    }

    // 4. Source-tier component (cap MAX_SOURCE_TIER). Max over all sources.
    let mut tier_score: u8 = 0;
    for source in &event.sources {
        let s = match source_tiers.get(&source.source_id) {
            Some(SourceTier::S) => 10,
            Some(SourceTier::A) => 7,
            Some(SourceTier::B) => 4,
            Some(SourceTier::Unknown) | None => 0,
        };
        if s > tier_score {
            tier_score = s;
        }
    }
    let source_tier = tier_score.min(MAX_SOURCE_TIER);
    if source_tier >= MAX_SOURCE_TIER {
        rank_reasons.push("major_research_institute".to_string());
    }

    // 5. People component (cap MAX_PEOPLE). Important scholars (fields/abel/
    //    wolf/crafoord tags) with Speaker/Lecturer → 10, Organizer/Panelist → 5;
    //    any non-TitleMention/Unknown role → 3. TitleMention and Unknown → 0.
    let mut people_score: u8 = 0;
    let mut people_winner: Option<(&str, PersonRole)> = None;
    for person in &event.people {
        let important = is_important_scholar(&person.scholar_tags);
        let s = person_signal(person.role, important);
        if s > people_score {
            people_score = s;
            people_winner = Some((person.canonical_name.as_str(), person.role));
        }
    }
    let people = people_score.min(MAX_PEOPLE);
    if people >= 5
        && let Some((name, role)) = people_winner
    {
        // Role-specific reason: the threshold (≥5) is reached only by an
        // important Speaker/Lecturer (signal 10) or an important
        // Organizer/Panelist (signal 5), so the label must reflect the
        // winner's actual role rather than defaulting to "important_speaker".
        let label = match role {
            PersonRole::Speaker | PersonRole::Lecturer => "important_speaker",
            PersonRole::Organizer => "important_organizer",
            PersonRole::Panelist => "important_panelist",
            _ => "important_person",
        };
        rank_reasons.push(format!("{}: {}", label, name));
    }

    // 6. Completeness component (cap MAX_COMPLETENESS).
    let mut completeness_sum: u8 = 0;
    if event.description.as_ref().is_some_and(|d| !d.is_empty()) {
        completeness_sum += 3;
    }
    if event
        .talks
        .iter()
        .any(|t| t.abstract_text.as_ref().is_some_and(|a| !a.is_empty()))
    {
        completeness_sum += 3;
    }
    if event.location.is_some() {
        completeness_sum += 2;
    }
    if event.talks.len() >= 3 {
        completeness_sum += 2;
    }
    let completeness = completeness_sum.min(MAX_COMPLETENESS);
    if completeness >= MAX_COMPLETENESS {
        rank_reasons.push("program_complete".to_string());
    }

    let total = f32::from(topic + media + access + source_tier + people + completeness);
    let components = ScoreComponents {
        topic,
        media,
        access,
        source_tier,
        people,
        completeness,
    };
    (total, components, rank_reasons)
}

/// Per-media signal: open video=25, reg-required video=18, open audio=15,
/// slides/lecture_notes=10, else 0.
fn media_signal(media_type: &MediaType, access: PublicAccess) -> u8 {
    match (media_type, access) {
        (MediaType::Video, PublicAccess::Open) => 25,
        (MediaType::Video, _) => 18,
        (MediaType::Audio, PublicAccess::Open) => 15,
        (MediaType::Audio, _) => 0,
        (MediaType::Slides, _) => 10,
        (MediaType::LectureNotes, _) => 10,
        _ => 0,
    }
}

/// Access signal: livestream=15, hybrid=12, recording_expected=10, else=0.
fn access_signal(online: OnlineAvailability) -> u8 {
    match online {
        OnlineAvailability::Livestream => 15,
        OnlineAvailability::Hybrid => 12,
        OnlineAvailability::RecordingExpected => 10,
        _ => 0,
    }
}

/// Whether `scholar_tags` mark an "important" scholar (Fields/Abel/Wolf/
/// Crafoord laureate). Exact case-insensitive match: scholar tags are
/// curator-controlled values from `config/scholars.toml` (e.g. "fields",
/// "wolf", "abel", "crafoord"), so a substring test would false-positive on
/// tags like "wolfram" containing "wolf".
fn is_important_scholar(scholar_tags: &[String]) -> bool {
    const MARKERS: [&str; 4] = ["fields", "abel", "wolf", "crafoord"];
    scholar_tags
        .iter()
        .any(|tag| MARKERS.iter().any(|m| tag.to_lowercase() == *m))
}

/// People signal: important Speaker/Lecturer → 10, important Organizer/Panelist
/// → 5, any non-TitleMention/Unknown role → 3, else 0.
fn person_signal(role: PersonRole, important: bool) -> u8 {
    match role {
        PersonRole::TitleMention | PersonRole::Unknown => 0,
        PersonRole::Speaker | PersonRole::Lecturer if important => 10,
        PersonRole::Organizer | PersonRole::Panelist if important => 5,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::{InterestWeights, ScoreComponents, score_event};
    use std::collections::HashMap;

    use crate::config::SourceTier;
    use crate::date::{DatePrecision, EventDate};
    use crate::model::{
        AccessInfo, Event, EventId, EventStatus, EventType, MediaId, MediaResource, MediaType,
        OnlineAvailability, PublicAccess, SourceEvidence,
    };
    use crate::people::{PersonHit, PersonRole};
    use crate::topics::TopicMatch;
    use url::Url;

    fn empty_source_evidence() -> SourceEvidence {
        SourceEvidence {
            source_id: String::new(),
            source_url: Url::parse("https://example.com").unwrap(),
            evidence: None,
            captured_at: None,
            native_id: None,
        }
    }

    fn empty_event() -> Event {
        Event {
            id: EventId(String::new()),
            title: String::new(),
            url: None,
            event_type: EventType::Unknown,
            status: EventStatus::Unknown,
            date: EventDate {
                start: None,
                end: None,
                timezone: None,
                original_text: String::new(),
                precision: DatePrecision::Unknown,
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
            score_components: ScoreComponents::default(),
            rank_reasons: Vec::new(),
            first_seen_at: None,
            last_seen_at: None,
        }
    }

    fn topic_match(topic_id: &str) -> TopicMatch {
        TopicMatch {
            topic_id: topic_id.into(),
            canonical_name: topic_id.into(),
            matched_text: topic_id.into(),
            confidence: 1.0,
        }
    }

    fn deligne_hit(role: PersonRole) -> PersonHit {
        PersonHit {
            canonical_name: "Pierre Deligne".into(),
            matched_text: "Deligne".into(),
            role,
            evidence: None,
            confidence: 0.5,
            scholar_tags: vec![
                "fields".into(),
                "wolf".into(),
                "abel".into(),
                "crafoord".into(),
            ],
        }
    }

    // RANK-001: topic score without interests → each topic contributes 15,
    // sum 30, capped at MAX_TOPIC (30).
    #[test]
    fn rank_001_topic_no_interests() {
        let mut event = empty_event();
        event.topics = vec![
            topic_match("arithmetic_geometry"),
            topic_match("number_theory"),
        ];
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (total, components, reasons) = score_event(&event, &tiers, None);
        assert_eq!(components.topic, 30);
        assert!((total - 30.0).abs() < f32::EPSILON);
        assert!(reasons.contains(&"matched_topic: arithmetic_geometry".to_string()));
        assert!(reasons.contains(&"matched_topic: number_theory".to_string()));
    }

    // RANK-001: topic score with zero interest weight → 7.5 rounds half up to 8.
    #[test]
    fn rank_001_topic_zero_weight() {
        let mut event = empty_event();
        event.topics = vec![
            topic_match("arithmetic_geometry"),
            topic_match("number_theory"),
        ];
        let mut weights = HashMap::new();
        weights.insert("arithmetic_geometry".to_string(), 0.0);
        let iw = InterestWeights(weights);
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, reasons) = score_event(&event, &tiers, Some(&iw));
        // arithmetic_geometry: 15 * 0.5 = 7.5 → round half up → 8
        // number_theory: 15 * 1.0 = 15
        assert_eq!(components.topic, 23);
        assert!(reasons.contains(&"matched_topic: arithmetic_geometry".to_string()));
    }

    // RANK-002: open video → media component 25 + recording reason.
    #[test]
    fn rank_002_open_video() {
        let mut event = empty_event();
        event.media = vec![MediaResource {
            id: MediaId("m1".into()),
            media_type: MediaType::Video,
            title: None,
            url: Url::parse("https://example.com/v").unwrap(),
            platform: None,
            public_access: PublicAccess::Open,
            published_at: None,
            source: empty_source_evidence(),
        }];
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, reasons) = score_event(&event, &tiers, None);
        assert_eq!(components.media, 25);
        assert!(reasons.contains(&"public_recording_available".to_string()));
    }

    // RANK-002: slides → media component 10.
    #[test]
    fn rank_002_slides() {
        let mut event = empty_event();
        event.media = vec![MediaResource {
            id: MediaId("m1".into()),
            media_type: MediaType::Slides,
            title: None,
            url: Url::parse("https://example.com/s").unwrap(),
            platform: None,
            public_access: PublicAccess::Open,
            published_at: None,
            source: empty_source_evidence(),
        }];
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, _) = score_event(&event, &tiers, None);
        assert_eq!(components.media, 10);
    }

    // RANK-002: no media → media component 0, no recording reason.
    #[test]
    fn rank_002_no_media() {
        let event = empty_event();
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, reasons) = score_event(&event, &tiers, None);
        assert_eq!(components.media, 0);
        assert!(!reasons.contains(&"public_recording_available".to_string()));
    }

    // RANK-003: title-only mention of an important scholar → no people boost.
    #[test]
    fn rank_003_title_mention_no_boost() {
        let mut event = empty_event();
        event.people = vec![deligne_hit(PersonRole::TitleMention)];
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, reasons) = score_event(&event, &tiers, None);
        assert_eq!(components.people, 0);
        assert!(
            !reasons.iter().any(|r| r.starts_with("important_speaker:")),
            "title-only mention must not produce important_speaker reason, got {reasons:?}"
        );
    }

    // RANK-003: important scholar as Speaker → people component 10 + reason.
    #[test]
    fn rank_003_speaker_important() {
        let mut event = empty_event();
        event.people = vec![deligne_hit(PersonRole::Speaker)];
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, reasons) = score_event(&event, &tiers, None);
        assert_eq!(components.people, 10);
        assert!(reasons.contains(&"important_speaker: Pierre Deligne".to_string()));
    }

    // Edge case: empty event → all-zero score and no rank reasons.
    #[test]
    fn empty_event_scores_zero() {
        let event = empty_event();
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (total, components, reasons) = score_event(&event, &tiers, None);
        assert!(total.abs() < f32::EPSILON);
        assert_eq!(components, ScoreComponents::default());
        assert!(reasons.is_empty());
    }

    // CORE-17: NaN and negative interest weights are clamped to [0, 1] (NaN →
    // 1.0 neutral) so a matched topic is never silently zeroed.
    #[test]
    fn rank_nan_weight_treated_as_neutral() {
        let mut event = empty_event();
        event.topics = vec![topic_match("arithmetic_geometry")];
        let mut weights = HashMap::new();
        weights.insert("arithmetic_geometry".to_string(), f64::NAN);
        let iw = InterestWeights(weights);
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, reasons) = score_event(&event, &tiers, Some(&iw));
        assert_eq!(components.topic, 15, "NaN weight → neutral 1.0 → contribution 15");
        assert!(reasons.contains(&"matched_topic: arithmetic_geometry".to_string()));
    }

    #[test]
    fn rank_negative_weight_clamped_to_zero() {
        let mut event = empty_event();
        event.topics = vec![topic_match("arithmetic_geometry")];
        let mut weights = HashMap::new();
        weights.insert("arithmetic_geometry".to_string(), -5.0);
        let iw = InterestWeights(weights);
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, _) = score_event(&event, &tiers, Some(&iw));
        assert_eq!(
            components.topic, 8,
            "negative weight clamped to 0 → 15 * 0.5 = 7.5 → 8"
        );
    }

    #[test]
    fn rank_inf_weight_clamped_to_one() {
        let mut event = empty_event();
        event.topics = vec![topic_match("arithmetic_geometry")];
        let mut weights = HashMap::new();
        weights.insert("arithmetic_geometry".to_string(), f64::INFINITY);
        let iw = InterestWeights(weights);
        let tiers: HashMap<String, SourceTier> = HashMap::new();
        let (_, components, _) = score_event(&event, &tiers, Some(&iw));
        assert_eq!(
            components.topic, 15,
            "inf weight clamped to 1.0 → 15 * 1.0 = 15"
        );
    }
}
