use super::catalog::{self, Case, MODULES};
use super::evidence::{self, Identity, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Local,
    Shadow,
    Full,
    Release,
    Preflight,
    Live,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub identity: Identity,
    pub runner_sha256: String,
    pub run_id: String,
    pub attempt: String,
    pub profile: Profile,
    pub full: bool,
    pub base: Option<String>,
    pub changed_paths: Vec<String>,
    pub reasons: Vec<String>,
    pub modules: BTreeSet<String>,
    pub shadow_modules: BTreeSet<String>,
    pub checks: BTreeSet<String>,
    pub cases: Vec<Case>,
    pub catalog_digest: String,
    pub dependency_graph: BTreeMap<String, BTreeSet<String>>,
    pub msrv: String,
}

fn msrv(metadata: &Value) -> Result<String> {
    let versions: BTreeSet<_> = metadata["packages"]
        .as_array()
        .ok_or("metadata packages missing")?
        .iter()
        .map(|package| {
            package["rust_version"]
                .as_str()
                .ok_or("workspace package MSRV missing")
        })
        .collect::<std::result::Result<_, _>>()?;
    if versions.len() != 1 {
        return Err("workspace packages must inherit one MSRV".into());
    }
    let version = versions.first().ok_or("workspace MSRV missing")?;
    let parts: Vec<_> = version.split('.').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("invalid workspace MSRV".into());
    }
    Ok(if parts.len() == 2 {
        format!("{version}.0")
    } else {
        (*version).to_owned()
    })
}

pub fn graph(metadata: &Value) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("metadata packages missing")?;
    let mut graph = BTreeMap::new();
    for package in packages {
        let name = package["name"].as_str().ok_or("package name missing")?;
        if !MODULES.contains(&name) || graph.contains_key(name) {
            return Err(format!("unregistered/duplicate module: {name}"));
        }
        let mut dependencies = BTreeSet::new();
        for dependency in package["dependencies"]
            .as_array()
            .ok_or("dependencies missing")?
        {
            // Union ALL dependency kinds, optional features and target conditions.
            // Never reuse the production-only architecture validator for selection.
            if !dependency["path"].is_null() {
                let dep = dependency["name"]
                    .as_str()
                    .ok_or("dependency name missing")?;
                if !MODULES.contains(&dep) {
                    return Err(format!("unregistered path dependency: {dep}"));
                }
                dependencies.insert(dep.to_owned());
            }
        }
        graph.insert(name.to_owned(), dependencies);
    }
    if graph.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != MODULES.iter().copied().collect()
    {
        return Err("workspace module catalog is incomplete".into());
    }
    Ok(graph)
}

pub fn closure(
    mut modules: BTreeSet<String>,
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    loop {
        let consumers: Vec<_> = graph
            .iter()
            .filter(|(_, dependencies)| !dependencies.is_disjoint(&modules))
            .map(|(module, _)| module.clone())
            .collect();
        let count = modules.len();
        modules.extend(consumers);
        if modules.len() == count {
            return modules;
        }
    }
}

pub fn select(
    paths: &[String],
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> (bool, BTreeSet<String>, Vec<String>) {
    let mut modules = BTreeSet::new();
    let mut reasons = Vec::new();
    let mut full = false;
    for path in paths {
        let owner = MODULES.iter().find(|module| {
            let prefix = if **module == "math_talk_radar" {
                "apps/cli/".to_owned()
            } else {
                format!("crates/{module}/")
            };
            path.starts_with(&prefix)
        });
        // A public module root, shared configuration or unknown input is not a
        // private implementation change. Shared fixtures flow to all consumers.
        let public = path.ends_with("Cargo.toml")
            || !path.ends_with(".rs")
            || path.contains("/fixtures/")
            || path.contains("/golden_data/")
            || path.ends_with("build.rs")
            || path.ends_with("/lib.rs")
            || path.contains("/model.rs")
            || path == "apps/cli/src/output.rs"
            || path == "apps/cli/src/cli.rs"
            || path.starts_with("crates/radar-core/src/")
            || path.starts_with("crates/radar-state/src/")
            || path.starts_with("apps/cli/src/lifecycle/")
            || path.starts_with("apps/cli/src/scan_engine")
            || path == "apps/cli/src/config_loader.rs"
            || path == "apps/cli/src/runtime.rs";
        match owner {
            Some(module) if !public => {
                modules.insert((*module).to_owned());
                reasons.push(format!("{path}: owner {module} and reverse consumers"));
            }
            _ => {
                full = true;
                reasons.push(format!(
                    "{path}: shared contract/authority/rule or unknown impact; full required"
                ));
            }
        }
    }
    if paths.is_empty() {
        reasons.push("no changed source paths; metadata and formatting still run".into());
    }
    if full {
        modules = MODULES.iter().map(|m| (*m).to_owned()).collect();
    }
    (full, closure(modules, graph), reasons)
}

pub fn checks(profile: &Profile, full: bool, modules: &BTreeSet<String>) -> BTreeSet<String> {
    if *profile == Profile::Live {
        return ["build".into(), "live".into()].into();
    }
    let mut checks: BTreeSet<String> = ["meta".into(), "quality".into()].into();
    if !modules.is_empty() {
        checks.insert("tests".into());
    }
    if full {
        checks.extend(["security", "coverage", "performance", "msrv"].map(str::to_owned));
    }
    if matches!(profile, Profile::Release | Profile::Preflight) {
        checks.extend(["build", "artifact", "attestation"].map(str::to_owned));
    }
    if *profile == Profile::Release {
        checks.insert("review".into());
    }
    checks
}

pub fn create(root: &Path, profile: Profile, base: Option<String>) -> Result<Plan> {
    let identity = evidence::identity(root)?;
    if matches!(profile, Profile::Release | Profile::Preflight) && identity.dirty {
        return Err("release/preflight requires a clean source checkout".into());
    }
    let cases = catalog::load(root)?;
    let metadata = crate::architecture::metadata(root)?;
    let graph = graph(&metadata)?;
    let msrv = msrv(&metadata)?;
    let mut changes = BTreeSet::new();
    let mut unknown_base = false;
    if let Some(base) = &base {
        match evidence::output(
            root,
            "git",
            &[
                "diff",
                "--name-only",
                "--no-renames",
                "-z",
                base,
                "HEAD",
                "--",
            ],
        ) {
            Ok(paths) => changes.extend(
                paths
                    .split('\0')
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned),
            ),
            Err(_) => unknown_base = true,
        }
        // A branch base must be an ancestor; a divergent comparison is not proof
        // of changes since a trusted baseline.
        if evidence::output(root, "git", &["merge-base", "--is-ancestor", base, "HEAD"]).is_err() {
            unknown_base = true;
        }
    } else {
        unknown_base = true;
    }
    for args in [
        vec!["diff", "HEAD", "--name-only", "--no-renames", "-z", "--"],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        changes.extend(
            evidence::output(root, "git", &args)?
                .split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_owned),
        );
    }
    let changed_paths: Vec<_> = changes.into_iter().collect();
    let (selected_full, shadow_modules, mut reasons) = select(&changed_paths, &graph);
    let ci_forces_full = std::env::var("GITHUB_REF")
        .is_ok_and(|r| r == "refs/heads/main" || r.starts_with("refs/tags/"))
        || std::env::var("GITHUB_EVENT_NAME")
            .is_ok_and(|e| ["schedule", "deployment", "workflow_dispatch"].contains(&e.as_str()));
    let full = profile != Profile::Live
        && (profile != Profile::Local || selected_full || unknown_base || ci_forces_full);
    if unknown_base {
        reasons.push("trusted ancestor base unavailable; full required".into());
    }
    if ci_forces_full {
        reasons.push("main/tag/scheduled/manual/deployment CI requires full".into());
    }
    if profile != Profile::Local {
        reasons.push(format!(
            "{profile:?} profile executes full baseline (live is separate)"
        ));
    }
    let modules = if profile == Profile::Live {
        BTreeSet::new()
    } else if full {
        MODULES.iter().map(|m| (*m).to_owned()).collect()
    } else {
        shadow_modules.clone()
    };
    Ok(Plan {
        schema_version: 1,
        identity,
        runner_sha256: evidence::file_hash(&std::env::current_exe().map_err(|e| e.to_string())?)?,
        run_id: std::env::var("GITHUB_RUN_ID").unwrap_or(format!(
            "local-{}-{}",
            evidence::now()?,
            std::process::id()
        )),
        attempt: std::env::var("GITHUB_RUN_ATTEMPT").unwrap_or_else(|_| "1".into()),
        checks: checks(&profile, full, &modules),
        profile,
        full,
        base,
        changed_paths,
        reasons,
        modules,
        shadow_modules,
        catalog_digest: catalog::digest(&cases)?,
        cases,
        dependency_graph: graph,
        msrv,
    })
}

pub fn validate(root: &Path, plan: &Plan) -> Result<()> {
    if plan.schema_version != 1 || plan.run_id.is_empty() || plan.attempt.is_empty() {
        return Err("invalid plan identity/version".into());
    }
    if plan.identity != evidence::identity(root)? {
        return Err("source/toolchain/flags changed since plan creation".into());
    }
    if plan.runner_sha256
        != evidence::file_hash(&std::env::current_exe().map_err(|e| e.to_string())?)?
    {
        return Err("runner differs from planned artifact".into());
    }
    // Workflow attempt is retry provenance, not immutable plan identity. A
    // later attempt may intentionally consume the verified control bundle from
    // an earlier attempt of the same GitHub run.
    if std::env::var("GITHUB_RUN_ID")
        .is_ok_and(|value| value.as_str() != plan.run_id.as_str())
    {
        return Err("wrong GITHUB_RUN_ID".into());
    }
    let recomputed = create(root, plan.profile.clone(), plan.base.clone())?;
    if plan.cases != recomputed.cases
        || plan.catalog_digest != recomputed.catalog_digest
        || plan.dependency_graph != recomputed.dependency_graph
        || plan.modules != recomputed.modules
        || plan.checks != recomputed.checks
        || plan.full != recomputed.full
        || plan.changed_paths != recomputed.changed_paths
        || plan.shadow_modules != recomputed.shadow_modules
        || plan.reasons != recomputed.reasons
        || plan.msrv != recomputed.msrv
    {
        return Err("plan differs from current catalog/dependency/selection rules".into());
    }
    Ok(())
}

pub fn selected_cases(plan: &Plan) -> Vec<&Case> {
    plan.cases
        .iter()
        .filter(|case| {
            plan.checks.contains(&case.check)
                && (case.check != "tests" || plan.modules.contains(&case.module))
                && (case.scope == "baseline"
                    || (case.scope == "release"
                        && matches!(plan.profile, Profile::Release | Profile::Preflight))
                    || (case.scope == "live" && plan.profile == Profile::Live))
        })
        .collect()
}
