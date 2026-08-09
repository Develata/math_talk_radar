//! Golden test harness for M1 core domain algorithms (§47).
//!
//! Loads config and golden data at compile time via `include_str!` — no
//! filesystem I/O at test time. Each submodule exposes a `run` function that
//! evaluates golden cases and returns statistics. The `#[test]` functions here
//! call those runners and assert the results, including the §47 aggregate
//! metrics (date accuracy ≥ 98%, scholar precision/recall ≥ 95%, role-protection
//! FP = 0).

mod dates;
mod dedup;
mod people;
mod ranking;
mod topics;

use radar_core::people::ScholarRecord;
use radar_core::topics::TopicRecord;
use serde::Deserialize;

// --- Compile-time embedded data ---------------------------------------------

const SCHOLARS_TOML: &str = include_str!("../../../../config/scholars.toml");
const TOPICS_TOML: &str = include_str!("../../../../config/topics.toml");
const DATES_TOML: &str = include_str!("../golden_data/dates.toml");
const DEDUP_TOML: &str = include_str!("../golden_data/dedup.toml");
const PEOPLE_TOML: &str = include_str!("../golden_data/people.toml");
const RANKING_TOML: &str = include_str!("../golden_data/ranking.toml");

// --- Config loaders ---------------------------------------------------------

#[derive(Deserialize)]
struct ScholarsConfig {
    scholars: Vec<ScholarRecord>,
}

#[derive(Deserialize)]
struct TopicsConfig {
    topics: Vec<TopicRecord>,
}

fn load_scholars() -> Vec<ScholarRecord> {
    let config: ScholarsConfig = toml::from_str(SCHOLARS_TOML).expect("scholars.toml parses");
    config.scholars
}

fn load_topics() -> Vec<TopicRecord> {
    let config: TopicsConfig = toml::from_str(TOPICS_TOML).expect("topics.toml parses");
    config.topics
}

// --- Per-domain golden tests ------------------------------------------------

#[test]
fn dates_golden() {
    let stats = dates::run(DATES_TOML);
    assert!(
        stats.total >= 50,
        "need >= 50 date cases, got {}",
        stats.total
    );
    assert_eq!(
        stats.passed,
        stats.total,
        "{} date case(s) failed",
        stats.total - stats.passed
    );
}

#[test]
fn people_golden() {
    let scholars = load_scholars();
    let stats = people::run(PEOPLE_TOML, &scholars);
    assert!(
        stats.total >= 60,
        "need >= 60 people cases, got {}",
        stats.total
    );
    assert!(
        stats.failures.is_empty(),
        "people golden failures:\n{}",
        stats.failures.join("\n")
    );
}

#[test]
fn topics_golden() {
    let topics = load_topics();
    topics::run(&topics);
}

#[test]
fn ranking_golden() {
    let stats = ranking::run(RANKING_TOML);
    assert!(
        stats.total >= 20,
        "need >= 20 ranking cases, got {}",
        stats.total
    );
    assert_eq!(
        stats.passed,
        stats.total,
        "{} ranking case(s) failed",
        stats.total - stats.passed
    );
    assert!(
        stats.failures.is_empty(),
        "ranking golden failures:\n{}",
        stats.failures.join("\n")
    );
}

#[test]
fn dedup_golden() {
    let stats = dedup::run(DEDUP_TOML);
    assert!(
        stats.total >= 30,
        "need >= 30 dedup pairs, got {}",
        stats.total
    );
    // §47: precision = 100% (no wrong merges — a wrong merge is a release blocker).
    assert_eq!(
        stats.false_positives,
        0,
        "dedup precision < 100%: {} wrong merge(s)\n{}",
        stats.false_positives,
        stats.failures.join("\n")
    );
    // §47: recall ≥ 90%.
    let recall_denom = stats.true_positives + stats.false_negatives;
    if recall_denom > 0 {
        let recall = stats.true_positives as f64 / recall_denom as f64;
        assert!(recall >= 0.90, "dedup recall {recall:.4} < 0.90");
    }
    assert!(
        stats.failures.is_empty(),
        "dedup golden failures:\n{}",
        stats.failures.join("\n")
    );
}

#[test]
fn rel_003_stable_deterministic_ids() {
    dedup::run_rel003();
}

// --- §47 aggregate metrics --------------------------------------------------

#[test]
fn section_47_metrics() {
    let scholars = load_scholars();
    let date_stats = dates::run(DATES_TOML);
    let people_stats = people::run(PEOPLE_TOML, &scholars);

    // Date accuracy ≥ 98%.
    let date_accuracy = date_stats.passed as f64 / date_stats.total.max(1) as f64;
    assert!(
        date_accuracy >= 0.98,
        "date accuracy {date_accuracy:.4} < 0.98"
    );

    // Scholar precision = TP / (TP + FP) ≥ 95%.
    let precision_denom = people_stats.tp + people_stats.fp;
    if precision_denom > 0 {
        let precision = people_stats.tp as f64 / precision_denom as f64;
        assert!(precision >= 0.95, "scholar precision {precision:.4} < 0.95");
    }

    // Scholar recall = TP / (TP + FN) ≥ 95%.
    let recall_denom = people_stats.tp + people_stats.fn_;
    if recall_denom > 0 {
        let recall = people_stats.tp as f64 / recall_denom as f64;
        assert!(recall >= 0.95, "scholar recall {recall:.4} < 0.95");
    }

    // Role-protection FP = 0 (no PER-003 concept-name case labeled Speaker).
    assert_eq!(
        people_stats.role_protection_fp, 0,
        "role-protection FP must be 0, got {}",
        people_stats.role_protection_fp
    );
}
