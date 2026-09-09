//! Conservative selection, run-local execution and fail-closed aggregation.
mod artifact;
pub(crate) mod catalog;
pub(super) mod evidence;
mod junit;
mod live;
mod plan;
mod run;
pub(crate) mod smoke;

use evidence::{Result, read_json, write_json};
use plan::{Plan, Profile};
use run::{Context, Receipt};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn options(args: &[String], allowed: &[&str]) -> Result<BTreeMap<String, String>> {
    let mut options = BTreeMap::new();
    let mut args = args.iter();
    while let Some(key) = args.next() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown option: {key}"));
        }
        let value = args
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{key} requires a value"))?;
        if options.insert(key.clone(), value.clone()).is_some() {
            return Err(format!("duplicate option: {key}"));
        }
    }
    Ok(options)
}

fn required<'a>(options: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    options
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("required option: {key}"))
}

fn safe_output(root: &Path, path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    let parent = absolute.parent().ok_or("output parent missing")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    // Outputs in source directories would change the input snapshot mid-run.
    // A fixed ignored tree also prevents arbitrary application-path writes.
    let target = root.join("target");
    if parent.starts_with(root) && !parent.starts_with(&target) {
        return Err("acceptance output inside checkout must be under target/".into());
    }
    Ok(parent.join(absolute.file_name().ok_or("output name missing")?))
}

pub fn cli(root: &Path, args: &[String]) -> Result<()> {
    let command = args
        .first()
        .map(String::as_str)
        .ok_or("usage: acceptance plan|run|summarize|verify-artifact")?;
    match command {
        "plan" => {
            let options = options(&args[1..], &["--profile", "--base", "--out"])?;
            let profile = match required(&options, "--profile")? {
                "local" => Profile::Local,
                "shadow" => Profile::Shadow,
                "full" => Profile::Full,
                "release" => Profile::Release,
                "preflight" => Profile::Preflight,
                "live" => Profile::Live,
                _ => return Err("profile must be local|shadow|full|release|preflight|live".into()),
            };
            let plan = plan::create(root, profile, options.get("--base").cloned())?;
            let output = safe_output(root, Path::new(required(&options, "--out")?))?;
            if output.exists() {
                return Err("plan output already exists; use a new run directory".into());
            }
            write_json(&output, &plan)?;
            println!(
                "acceptance plan {}: modules {:?}, checks {:?}",
                output.display(),
                plan.modules,
                plan.checks
            );
            for reason in &plan.reasons {
                println!("  {reason}");
            }
        }
        "run" => {
            let options = options(
                &args[1..],
                &["--plan", "--shard", "--out", "--artifacts", "--review"],
            )?;
            let plan_path = Path::new(required(&options, "--plan")?);
            let plan: Plan = read_json(plan_path)?;
            plan::validate(root, &plan)?;
            let shard = required(&options, "--shard")?;
            let output = safe_output(root, Path::new(required(&options, "--out")?))?;
            if shard == "all" {
                let mut errors = Vec::new();
                for shard in [
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
                ] {
                    if plan.checks.contains(shard)
                        && let Err(error) =
                            run_shard(root, plan_path, &plan, shard, &output, &options)
                    {
                        errors.push(error);
                    }
                }
                if !errors.is_empty() {
                    return Err(errors.join("\n"));
                }
            } else {
                run_shard(root, plan_path, &plan, shard, &output, &options)?;
            }
        }
        "summarize" => {
            let options = options(&args[1..], &["--plan", "--receipts", "--out", "--stage"])?;
            let plan_path = Path::new(required(&options, "--plan")?);
            let plan: Plan = read_json(plan_path)?;
            plan::validate(root, &plan)?;
            let stage = options
                .get("--stage")
                .map(String::as_str)
                .unwrap_or("final");
            if !["baseline", "pre-attestation", "final"].contains(&stage) {
                return Err("invalid summary stage".into());
            }
            let summary = summarize(
                &plan,
                &evidence::file_hash(plan_path)?,
                Path::new(required(&options, "--receipts")?),
                stage,
            )?;
            let output = safe_output(root, Path::new(required(&options, "--out")?))?;
            if output.exists() {
                return Err("summary output already exists; use a new output path".into());
            }
            write_json(&output, &summary)?;
            if summary["status"] != "pass" && summary["status"] != "advisory" {
                return Err(format!("acceptance fan-in failed: {}", summary["errors"]));
            }
        }
        "verify-artifact" => {
            let options = options(&args[1..], &["--plan", "--artifacts"])?;
            let path = Path::new(required(&options, "--plan")?);
            let plan: Plan = read_json(path)?;
            plan::validate(root, &plan)?;
            artifact::verify_manifest(
                &plan,
                &evidence::file_hash(path)?,
                Path::new(required(&options, "--artifacts")?),
            )?;
        }
        _ => return Err(format!("unknown acceptance command: {command}")),
    }
    Ok(())
}

fn run_shard(
    root: &Path,
    plan_path: &Path,
    plan: &Plan,
    shard: &str,
    output: &Path,
    options: &BTreeMap<String, String>,
) -> Result<()> {
    if !plan.checks.contains(shard) {
        return Err(format!("shard {shard} was not selected"));
    }
    let directory = output.join(shard);
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    std::fs::create_dir(&directory).map_err(|e| format!("new shard directory required: {e}"))?;
    let receipt = Receipt {
        schema_version: 1,
        plan_sha256: evidence::file_hash(plan_path)?,
        run_id: plan.run_id.clone(),
        attempt: plan.attempt.clone(),
        shard: shard.into(),
        status: "running".into(),
        error: None,
        commands: Vec::new(),
        attachments: BTreeMap::new(),
        tests: Vec::new(),
        cases: Vec::new(),
        detail: Value::Null,
    };
    write_json(&directory.join("receipt.json"), &receipt)?;
    let mut context = Context {
        root,
        directory: &directory,
        plan,
        receipt,
    };
    let artifacts = options
        .get("--artifacts")
        .map(PathBuf::from)
        .unwrap_or_else(|| output.join("build/artifacts"));
    let result = match shard {
        "build" => artifact::build(&mut context),
        "artifact" => artifact::verify(&mut context, &artifacts),
        "review" => (|| {
            let review: Value = read_json(Path::new(required(options, "--review")?))?;
            artifact::review(plan, &review)?;
            write_json(&directory.join("review.json"), &review)?;
            context.attach("review.json")
        })(),
        "attestation" => artifact::attest(&mut context, &artifacts),
        "live" => live::run(&mut context, &artifacts),
        _ => context.standard(shard),
    }
    .and_then(|()| plan::validate(root, plan));
    context.receipt.status = if result.is_ok() {
        if shard == "live" { "advisory" } else { "pass" }
    } else {
        "fail"
    }
    .into();
    context.receipt.error = result.as_ref().err().cloned();
    if result.is_ok() {
        context.receipt.cases = plan::selected_cases(plan)
            .into_iter()
            .filter(|case| case.check == shard)
            .map(|case| case.id.clone())
            .collect();
    }
    write_json(&directory.join("receipt.json"), &context.receipt)?;
    println!(
        "acceptance {shard}: {} ({})",
        context.receipt.status,
        directory.display()
    );
    result
}

fn summarize(plan: &Plan, plan_digest: &str, directory: &Path, stage: &str) -> Result<Value> {
    let mut errors = Vec::new();
    let mut statuses = BTreeMap::new();
    let expected: BTreeSet<_> =
        plan.checks
            .iter()
            .filter(|check| match stage {
                "baseline" => !["build", "artifact", "review", "attestation", "live"]
                    .contains(&check.as_str()),
                "pre-attestation" => check.as_str() != "attestation",
                _ => true,
            })
            .cloned()
            .collect();
    if expected.is_empty() {
        errors.push("summary stage has no required checks".into());
    }
    for shard in &expected {
        let path = directory.join(shard);
        let result = (|| {
            let receipt: Receipt = read_json(&path.join("receipt.json"))?;
            run::validate_receipt(plan, plan_digest, shard, &path, &receipt)?;
            if shard == "review" {
                artifact::review(plan, &read_json(&path.join("review.json"))?)?;
            }
            if shard == "build" {
                artifact::verify_manifest(plan, plan_digest, &path.join("artifacts"))?;
            }
            if ["artifact", "attestation", "live"].contains(&shard.as_str()) {
                let manifest = artifact::verify_manifest(
                    plan,
                    plan_digest,
                    &directory.join("build/artifacts"),
                )?;
                if receipt.detail["binary_sha256"] != manifest.files[artifact::BINARY] {
                    return Err("consumer receipt belongs to another binary".into());
                }
            }
            Ok::<_, String>(())
        })();
        statuses.insert(
            shard.clone(),
            if result.is_ok() {
                if shard == "live" { "advisory" } else { "pass" }
            } else {
                "fail"
            },
        );
        if let Err(error) = result {
            errors.push(error);
        }
    }
    // Extra unknown shard directories are also catalog drift, not ignorable data.
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir()
            && !plan
                .checks
                .contains(&entry.file_name().to_string_lossy().into_owned())
        {
            errors.push(format!(
                "unexpected shard directory: {}",
                entry.path().display()
            ));
        }
    }
    let selected: BTreeSet<_> = plan::selected_cases(plan)
        .into_iter()
        .map(|case| &case.id)
        .collect();
    let cases: BTreeMap<_, _> = plan
        .cases
        .iter()
        .map(|case| {
            let status = if !selected.contains(&case.id) {
                "not-selected"
            } else if !expected.contains(&case.check) {
                "not-run"
            } else if statuses.get(&case.check) == Some(&"pass") {
                "pass"
            } else if statuses.get(&case.check) == Some(&"advisory") {
                "advisory"
            } else {
                "not-passed"
            };
            (&case.id, status)
        })
        .collect();
    Ok(
        json!({"schema_version": 1, "plan_sha256": plan_digest, "identity": plan.identity,
        "run_id": plan.run_id, "attempt": plan.attempt, "profile": plan.profile, "stage": stage,
        "status": if errors.is_empty() { if plan.profile == Profile::Live { "advisory" } else { "pass" } } else { "fail" }, "checks": statuses,
        "cases": cases, "errors": errors}),
    )
}

#[cfg(test)]
mod tests;
