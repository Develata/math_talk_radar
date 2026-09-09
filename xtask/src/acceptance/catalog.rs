use super::evidence::{Result, hash};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const MODULES: &[&str] = &[
    "math_talk_radar",
    "radar-adapters",
    "radar-core",
    "radar-fetch",
    "radar-state",
    "xtask",
];
pub const CHECKS: &[&str] = &[
    "meta",
    "quality",
    "tests",
    "security",
    "coverage",
    "performance",
    "msrv",
    "build",
    "artifact",
    "review",
    "attestation",
    "live",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub check: String,
    pub module: String,
    pub binary: String,
    pub test: String,
    pub scope: String,
    pub gate: String,
}

fn is_case_id(id: &str) -> bool {
    id.split_once('-').is_some_and(|(prefix, suffix)| {
        !prefix.is_empty()
            && prefix.bytes().all(|b| b.is_ascii_uppercase())
            && suffix.len() == 3
            && suffix.bytes().all(|b| b.is_ascii_digit())
    })
}

pub fn load(root: &Path) -> Result<Vec<Case>> {
    let directory = root.join("docs/acceptance-cases");
    let mut required = BTreeMap::new();
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        for line in text.lines() {
            let cells: Vec<_> = line.split('|').map(str::trim).collect();
            if cells.len() >= 5 && is_case_id(cells[1]) {
                let id = cells[1].to_owned();
                if required.insert(id.clone(), cells[3].to_owned()).is_some() {
                    return Err(format!("duplicate case definition: {id}"));
                }
            }
        }
    }
    let text = std::fs::read_to_string(root.join("docs/registry/acceptance-matrix.tsv"))
        .map_err(|e| e.to_string())?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Vec<_> = lines
        .next()
        .ok_or("empty acceptance matrix")?
        .split('\t')
        .collect();
    let columns = [
        "case_id",
        "requirement",
        "plan_ref",
        "test_surface",
        "check",
        "module",
        "binary",
        "test",
        "scope",
        "gate",
    ];
    if header != columns {
        return Err("acceptance matrix columns do not match the catalog schema".into());
    }
    let mut cases = BTreeMap::new();
    let mut plans = BTreeSet::new();
    for line in lines {
        let row: Vec<_> = line.split('\t').collect();
        if row.len() != columns.len() {
            return Err("malformed acceptance row".into());
        }
        let [
            id,
            requirement,
            plan,
            surface,
            check,
            module,
            binary,
            test,
            scope,
            gate,
        ] = row[..]
        else {
            return Err("malformed acceptance row".into());
        };
        if !is_case_id(id)
            || requirement.is_empty()
            || surface.is_empty()
            || !CHECKS.contains(&check)
            || !["baseline", "release", "live"].contains(&scope)
            || !["hard", "advisory"].contains(&gate)
            || !root.join(plan).is_file()
        {
            return Err(format!("invalid acceptance mapping: {id}"));
        }
        if check == "tests"
            && (!MODULES.contains(&module)
                || binary.is_empty()
                || test.is_empty()
                || !(binary == module || binary.starts_with(&format!("{module}::"))))
        {
            return Err(format!(
                "{id}: tests require a module and exact binary/test ID"
            ));
        }
        if check != "tests" && (!module.is_empty() || !binary.is_empty() || !test.is_empty()) {
            return Err(format!("{id}: non-test check cannot claim a test ID"));
        }
        let expected_scope = match check {
            "build" | "artifact" | "review" | "attestation" => "release",
            "live" => "live",
            _ => "baseline",
        };
        if scope != expected_scope {
            return Err(format!("{id}: check {check} cannot satisfy scope {scope}"));
        }
        if (scope == "live") != (gate == "advisory") {
            return Err(format!("{id}: only live checks may be advisory"));
        }
        plans.insert(plan.to_owned());
        let case = Case {
            id: id.into(),
            check: check.into(),
            module: module.into(),
            binary: binary.into(),
            test: test.into(),
            scope: scope.into(),
            gate: gate.into(),
        };
        if cases.insert(id.to_owned(), case).is_some() {
            return Err(format!("duplicate matrix case: {id}"));
        }
    }
    let mapped: BTreeMap<_, _> = cases
        .iter()
        .map(|(id, case)| (id.clone(), case.gate.clone()))
        .collect();
    if required.is_empty() || mapped != required {
        return Err(format!(
            "case definitions and registry differ (including gate): required {required:?}, mapped {mapped:?}"
        ));
    }
    for entry in std::fs::read_dir(root.join("docs/plan")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == "md") {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy();
            if !plans.contains(relative.as_ref()) {
                return Err(format!("plan has no acceptance mapping: {relative}"));
            }
        }
    }
    Ok(cases.into_values().collect())
}

pub fn digest(cases: &[Case]) -> Result<String> {
    Ok(hash(&serde_json::to_vec(cases).map_err(|e| e.to_string())?))
}

pub fn validate(root: &Path) -> Vec<String> {
    load(root).err().into_iter().collect()
}
