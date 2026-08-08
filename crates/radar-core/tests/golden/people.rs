//! Scholar matcher golden tests — parses `people.toml` and evaluates each case
//! against `radar_core::people::match_scholars`.

use radar_core::people::{MatchContext, PersonHit, PersonRole, ScholarRecord, match_scholars};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Cases {
    cases: Vec<PeopleCase>,
}

#[derive(Deserialize)]
pub(crate) struct PeopleCase {
    id: String,
    input: String,
    context: String,
    expected_match: bool,
    expected_canonical: String,
    #[serde(default)]
    expected_role: Option<String>,
    #[serde(default)]
    expected_confidence: Option<f32>,
    #[serde(default)]
    extra_scholars: Vec<String>,
    #[serde(default)]
    empty_scholars: bool,
    #[serde(default)]
    expected_hit_count: Option<usize>,
}

#[derive(Debug, Default)]
pub struct PeopleStats {
    pub total: usize,
    pub tp: usize,
    pub fp: usize,
    pub fn_: usize,
    pub role_protection_fp: usize,
    pub failures: Vec<String>,
}

/// Inline scholar record for `benedict-gross`, used alongside the seeded
/// scholars for PER-003 concept-name cases. Not in the config file.
fn benedict_gross() -> ScholarRecord {
    ScholarRecord {
        id: "benedict-gross".into(),
        canonical_name: "Benedict Gross".into(),
        aliases: vec!["Gross".into()],
        tags: vec![],
    }
}

fn parse_role(s: &str) -> PersonRole {
    match s {
        "speaker" => PersonRole::Speaker,
        "lecturer" => PersonRole::Lecturer,
        "organizer" => PersonRole::Organizer,
        "participant" => PersonRole::Participant,
        "panelist" => PersonRole::Panelist,
        "honoree" => PersonRole::Honoree,
        "series_namesake" => PersonRole::SeriesNamesake,
        "title_mention" => PersonRole::TitleMention,
        "unknown" => PersonRole::Unknown,
        _ => PersonRole::Unknown,
    }
}

fn parse_context(s: &str) -> MatchContext {
    match s {
        "body_text" => MatchContext::BodyText,
        "title_text" => MatchContext::TitleText,
        other => {
            let prefix = "structured_field_";
            let role_str = other.strip_prefix(prefix).unwrap_or("unknown");
            MatchContext::StructuredField(parse_role(role_str))
        }
    }
}

fn build_scholar_list(case: &PeopleCase, seeded: &[ScholarRecord]) -> Vec<ScholarRecord> {
    if case.empty_scholars {
        return Vec::new();
    }
    let mut list: Vec<ScholarRecord> = seeded.to_vec();
    for id in &case.extra_scholars {
        if id == "benedict-gross" {
            list.push(benedict_gross());
        }
    }
    list
}

fn confidence_match(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 1e-6
}

pub fn run(data: &str, seeded: &[ScholarRecord]) -> PeopleStats {
    let parsed: Cases = toml::from_str(data).expect("people.toml parses");
    let total = parsed.cases.len();
    let mut stats = PeopleStats {
        total,
        ..Default::default()
    };

    for case in &parsed.cases {
        let scholars = build_scholar_list(case, seeded);
        let context = parse_context(&case.context);
        let hits = match_scholars(&case.input, &scholars, context);

        // Role-protection: PER-003 concept-name cases must never yield Speaker.
        if case.id.starts_with("PER-003")
            && hits
                .iter()
                .any(|h: &PersonHit| h.role == PersonRole::Speaker)
        {
            stats.role_protection_fp += 1;
            stats
                .failures
                .push(format!("{}: PER-003 case produced Speaker role", case.id));
        }

        // Check expected_hit_count if specified.
        if let Some(expected_count) = case.expected_hit_count {
            if hits.len() != expected_count {
                stats.failures.push(format!(
                    "{}: expected {} hits, got {}",
                    case.id,
                    expected_count,
                    hits.len()
                ));
                // Count as FP or FN depending on expected_match.
                if case.expected_match {
                    stats.fn_ += 1;
                } else {
                    stats.fp += 1;
                }
                continue;
            }
        }

        let found = hits
            .iter()
            .find(|h| h.canonical_name == case.expected_canonical);

        match (case.expected_match, found) {
            (true, Some(hit)) => {
                let role_ok = case
                    .expected_role
                    .as_ref()
                    .map(|r| hit.role == parse_role(r))
                    .unwrap_or(true);
                let conf_ok = case
                    .expected_confidence
                    .map(|c| confidence_match(hit.confidence, c))
                    .unwrap_or(true);
                if role_ok && conf_ok {
                    stats.tp += 1;
                } else {
                    stats.fn_ += 1;
                    stats.failures.push(format!(
                        "{}: expected {:?} role={:?} conf={:?}, got role={:?} conf={}",
                        case.id,
                        case.expected_canonical,
                        case.expected_role,
                        case.expected_confidence,
                        hit.role,
                        hit.confidence
                    ));
                }
            }
            (true, None) => {
                stats.fn_ += 1;
                stats.failures.push(format!(
                    "{}: expected match for {:?} but none found (hits={:?})",
                    case.id, case.expected_canonical, hits
                ));
            }
            (false, Some(_)) => {
                stats.fp += 1;
                stats.failures.push(format!(
                    "{}: unexpected match for {:?} (hits={:?})",
                    case.id, case.expected_canonical, hits
                ));
            }
            (false, None) => {
                // True negative — not counted in precision/recall.
            }
        }
    }

    stats
}
