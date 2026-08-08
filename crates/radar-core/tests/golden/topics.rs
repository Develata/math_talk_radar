//! Topic matcher golden tests — exercises `radar_core::topics::match_topics`
//! against the 8 canonical topics loaded from `config/topics.toml`.

use radar_core::topics::{TopicMatch, TopicRecord, match_topics};

fn has_match(result: &[TopicMatch], topic_id: &str) -> bool {
    result.iter().any(|m| m.topic_id == topic_id)
}

fn has_match_conf(result: &[TopicMatch], topic_id: &str, confidence: f32) -> bool {
    result
        .iter()
        .any(|m| m.topic_id == topic_id && (m.confidence - confidence).abs() < 1e-6)
}

pub fn run(topics: &[TopicRecord]) {
    // Canonical name match → confidence 1.0.
    let r = match_topics("Arithmetic Geometry conference", topics);
    assert!(
        has_match_conf(&r, "arithmetic_geometry", 1.0),
        "canonical name match: {r:?}"
    );

    // Alias match → confidence 0.8.
    let r = match_topics("Workshop on Shimura varieties", topics);
    assert!(
        has_match_conf(&r, "arithmetic_geometry", 0.8),
        "alias match: {r:?}"
    );

    // Multiple topics match independently.
    let r = match_topics("arithmetic geometry and algebraic geometry", topics);
    assert!(
        has_match(&r, "arithmetic_geometry") && has_match(&r, "algebraic_geometry"),
        "multi-topic match: {r:?}"
    );

    // No match.
    let r = match_topics("no math topics here", topics);
    assert!(r.is_empty(), "no-match case: {r:?}");

    // Single-word alias uses word-boundary matching (analysis matches).
    let r = match_topics("a course on analysis", topics);
    assert!(has_match(&r, "analysis"), "word-boundary match: {r:?}");

    // Word-boundary prevents partial match (analysis ≠ psychoanalysis).
    let r = match_topics("psychoanalysis is not math", topics);
    assert!(r.is_empty(), "word-boundary prevention: {r:?}");

    // Alias "random graphs" for probability.
    let r = match_topics("random graphs seminar", topics);
    assert!(has_match(&r, "probability"), "alias random graphs: {r:?}");

    // Alias "quantum field theory" for mathematical_physics.
    let r = match_topics("quantum field theory workshop", topics);
    assert!(has_match(&r, "mathematical_physics"), "alias QFT: {r:?}");

    // Alias "Lie theory" for representation_theory.
    let r = match_topics("Lie theory study group", topics);
    assert!(
        has_match(&r, "representation_theory"),
        "alias Lie theory: {r:?}"
    );

    // Alias "homotopy theory" for topology.
    let r = match_topics("homotopy theory talks", topics);
    assert!(has_match(&r, "topology"), "alias homotopy: {r:?}");

    // Dedup: canonical beats alias (both present → confidence 1.0).
    let r = match_topics("arithmetic geometry and Shimura varieties", topics);
    assert_eq!(r.len(), 1, "dedup: {r:?}");
    assert!(
        has_match_conf(&r, "arithmetic_geometry", 1.0),
        "dedup canonical wins: {r:?}"
    );

    // Empty topics list.
    let r = match_topics("anything", &[]);
    assert!(r.is_empty(), "empty topics: {r:?}");

    // All 8 topics match a text mentioning all canonical names.
    let r = match_topics(
        "arithmetic geometry number theory algebraic geometry probability \
         mathematical physics representation theory analysis topology",
        topics,
    );
    assert_eq!(r.len(), 8, "all 8 topics: {r:?}");

    // Number theory canonical match.
    let r = match_topics("Number Theory conference", topics);
    assert!(
        has_match_conf(&r, "number_theory", 1.0),
        "number theory canonical: {r:?}"
    );

    // PDE alias for analysis.
    let r = match_topics("PDE and harmonic analysis", topics);
    assert!(has_match(&r, "analysis"), "PDE alias: {r:?}");

    // "schemes" alias for algebraic_geometry.
    let r = match_topics("schemes and sheaves", topics);
    assert!(has_match(&r, "algebraic_geometry"), "schemes alias: {r:?}");
}
