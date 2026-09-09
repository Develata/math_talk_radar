use super::artifact::{self, BINARY};
use super::evidence::{self, Result};
use super::run::Context;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

pub fn classify(code: Option<i32>, stdout: &str) -> Result<Value> {
    match code {
        Some(4) if stdout.trim().is_empty() => Ok(
            json!({"outcome": "unavailable", "usable_sources": 0, "total_sources": null, "health_ratio": null}),
        ),
        Some(0) => {
            let output: Value = serde_json::from_str(stdout)
                .map_err(|e| format!("malformed live scan JSON: {e}"))?;
            if output["schema_version"] != "1.0" {
                return Err("live scan schema mismatch".into());
            }
            let health = output["source_health"]
                .as_array()
                .ok_or("live source health missing")?;
            if health.is_empty() {
                return Err("live scan returned no source health".into());
            }
            let statuses = [
                "ok",
                "partial",
                "timeout",
                "http_error",
                "parse_error",
                "robots_denied",
                "dynamic_unsupported",
                "budget_exhausted",
                "disabled",
            ];
            for source in health {
                if source["status"]
                    .as_str()
                    .is_none_or(|status| !statuses.contains(&status))
                {
                    return Err("unknown live source status".into());
                }
            }
            let active: Vec<_> = health
                .iter()
                .filter(|source| source["status"] != "disabled")
                .collect();
            if active.is_empty() {
                return Err("live scan has no enabled source".into());
            }
            let usable = active
                .iter()
                .filter(|source| source["status"] == "ok" || source["status"] == "partial")
                .count();
            Ok(
                json!({"outcome": "observed", "usable_sources": usable, "total_sources": active.len(), "health_ratio": usable as f64 / active.len() as f64, "source_health": health}),
            )
        }
        _ => Err(format!(
            "live scan command failed with {code:?}; this is not an advisory source outage"
        )),
    }
}

pub fn run(context: &mut Context<'_>, directory: &Path) -> Result<()> {
    let manifest =
        artifact::verify_manifest(context.plan, &context.receipt.plan_sha256, directory)?;
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let environment: BTreeMap<_, _> = ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"]
        .into_iter()
        .map(|key| {
            (
                key.to_owned(),
                temporary.path().join(key).display().to_string(),
            )
        })
        .collect();
    let binary = directory.join(BINARY).display().to_string();
    let scan = evidence::execute_with_env(
        context.root,
        context.directory,
        0,
        &[
            binary.clone(),
            "scan".into(),
            "--no-state".into(),
            "--format".into(),
            "json".into(),
        ],
        &environment,
    )?;
    let stdout =
        std::fs::read_to_string(context.directory.join(&scan.stdout)).map_err(|e| e.to_string())?;
    let result = classify(scan.exit_code, &stdout);
    context.receipt.commands.push(scan);
    let health = result?;
    let doctor = evidence::execute_with_env(
        context.root,
        context.directory,
        1,
        &[binary, "doctor".into(), "--json".into()],
        &environment,
    )?;
    let output: Value = evidence::read_json(&context.directory.join(&doctor.stdout))?;
    let success = doctor.exit_code == Some(0) && output["schema_version"] == "1.0";
    context.receipt.commands.push(doctor);
    if !success {
        return Err("doctor execution/schema failure".into());
    }
    context.receipt.detail = json!({"binary_sha256": manifest.files[BINARY], "health": health});
    artifact::verify_manifest(context.plan, &context.receipt.plan_sha256, directory)?;
    Ok(())
}
