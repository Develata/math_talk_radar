use super::evidence::{self, Identity, Result};
use super::plan::{Plan, Profile};
use super::run::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const TARGET: &str = "x86_64-unknown-linux-musl";
pub const BINARY: &str = "math_talk_radar-x86_64-unknown-linux-musl";
pub const RUNNER: &str = "acceptance-runner-x86_64-unknown-linux-musl";
pub const IMAGE: &str =
    "ubuntu:22.04@sha256:2edbbc5dc405e9612ba3584ce95480277e3eb374407b5505fe26f17df77c7dbc";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub identity: Identity,
    pub plan_sha256: String,
    pub target: String,
    pub profile: String,
    pub features: Vec<String>,
    pub files: BTreeMap<String, String>,
}

fn executable(messages: &str, name: &str) -> Result<PathBuf> {
    let mut found = Vec::new();
    for line in messages.lines() {
        let message: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == name
            && let Some(path) = message["executable"].as_str()
        {
            found.push(PathBuf::from(path));
        }
    }
    match found.as_slice() {
        [path] => Ok(path.clone()),
        _ => Err(format!("expected exactly one build artifact for {name}")),
    }
}

pub fn build(context: &mut Context<'_>) -> Result<()> {
    let output = context.command(
        [
            "cargo",
            "build",
            "--offline",
            "--locked",
            "--release",
            "--target",
            TARGET,
            "-p",
            "math_talk_radar",
            "-p",
            "xtask",
            "--message-format=json",
        ]
        .map(str::to_owned)
        .into(),
    )?;
    let messages =
        std::fs::read_to_string(context.directory.join(output)).map_err(|e| e.to_string())?;
    let artifacts = context.directory.join("artifacts");
    std::fs::create_dir(&artifacts).map_err(|e| e.to_string())?;
    let mut files = BTreeMap::new();
    for (name, destination) in [("math_talk_radar", BINARY), ("xtask", RUNNER)] {
        std::fs::copy(executable(&messages, name)?, artifacts.join(destination))
            .map_err(|e| e.to_string())?;
        files.insert(
            destination.into(),
            evidence::file_hash(&artifacts.join(destination))?,
        );
    }
    let checksum = format!("{}  {BINARY}\n", files[BINARY]);
    std::fs::write(artifacts.join(format!("{BINARY}.sha256")), checksum)
        .map_err(|e| e.to_string())?;
    files.insert(
        format!("{BINARY}.sha256"),
        evidence::file_hash(&artifacts.join(format!("{BINARY}.sha256")))?,
    );
    let manifest = Manifest {
        schema_version: 1,
        identity: context.plan.identity.clone(),
        plan_sha256: context.receipt.plan_sha256.clone(),
        target: TARGET.into(),
        profile: "release".into(),
        features: Vec::new(),
        files,
    };
    evidence::write_json(&artifacts.join("manifest.json"), &manifest)?;
    for name in manifest
        .files
        .keys()
        .chain(std::iter::once(&"manifest.json".to_owned()))
    {
        context.attach(&format!("artifacts/{name}"))?;
    }
    context.receipt.detail = json!({"target": TARGET, "profile": "release", "manifest_sha256": evidence::file_hash(&artifacts.join("manifest.json"))?});
    Ok(())
}

pub fn verify_manifest(plan: &Plan, plan_digest: &str, directory: &Path) -> Result<Manifest> {
    let manifest: Manifest = evidence::read_json(&directory.join("manifest.json"))?;
    let expected_files: std::collections::BTreeSet<_> = [
        BINARY.to_owned(),
        RUNNER.to_owned(),
        format!("{BINARY}.sha256"),
    ]
    .into();
    if manifest.schema_version != 1
        || manifest.identity != plan.identity
        || manifest.plan_sha256 != plan_digest
        || manifest.target != TARGET
        || manifest.profile != "release"
        || !manifest.features.is_empty()
        || manifest
            .files
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            != expected_files
    {
        return Err("artifact provenance does not match the plan/build tuple".into());
    }
    for (name, hash) in &manifest.files {
        if std::fs::symlink_metadata(directory.join(name))
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
            || evidence::file_hash(&directory.join(name))? != *hash
        {
            return Err(format!("artifact checksum mismatch: {name}"));
        }
    }
    if std::fs::read_to_string(directory.join(format!("{BINARY}.sha256")))
        .map_err(|e| e.to_string())?
        != format!("{}  {BINARY}\n", manifest.files[BINARY])
    {
        return Err("checksum asset content mismatch".into());
    }
    Ok(manifest)
}

pub fn verify(context: &mut Context<'_>, directory: &Path) -> Result<()> {
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let manifest = verify_manifest(context.plan, &context.receipt.plan_sha256, &directory)?;
    context.command(vec![
        std::env::current_exe()
            .map_err(|e| e.to_string())?
            .display()
            .to_string(),
        "--workspace-root".into(),
        context.root.display().to_string(),
        "static-release".into(),
        directory.join(BINARY).display().to_string(),
    ])?;
    let bytes = std::fs::metadata(directory.join(BINARY))
        .map_err(|e| e.to_string())?
        .len();
    if bytes > 30 * 1024 * 1024 {
        return Err("musl artifact exceeds 30 MiB".into());
    }
    let output = context.directory.join("smoke.json");
    for path in [&directory, &context.directory.to_owned()] {
        if path.as_os_str().as_encoded_bytes().contains(&b',') {
            return Err("Docker mount path contains a comma".into());
        }
    }
    context.command(vec![
        "docker".into(),
        "run".into(),
        "--rm".into(),
        "--network=none".into(),
        "--read-only".into(),
        "--tmpfs".into(),
        "/tmp:rw,nosuid".into(),
        "--mount".into(),
        format!(
            "type=bind,source={},target=/artifacts,readonly",
            directory.display()
        ),
        "--mount".into(),
        format!(
            "type=bind,source={},target=/evidence",
            context.directory.display()
        ),
        IMAGE.into(),
        format!("/artifacts/{RUNNER}"),
        "artifact-smoke".into(),
        format!("/artifacts/{BINARY}"),
        manifest.files[BINARY].clone(),
        "/evidence/smoke.json".into(),
    ])?;
    let smoke: Value = evidence::read_json(&output)?;
    if smoke["status"] != "pass"
        || smoke["binary_sha256"] != manifest.files[BINARY]
        || smoke["os_release"]
            .as_str()
            .is_none_or(|os| !os.contains("VERSION_ID=\"22.04\""))
    {
        return Err("clean Ubuntu smoke result does not match the artifact".into());
    }
    verify_manifest(context.plan, &context.receipt.plan_sha256, &directory)?;
    std::fs::copy(
        directory.join("manifest.json"),
        context.directory.join("manifest.json"),
    )
    .map_err(|e| e.to_string())?;
    context.attach("manifest.json")?;
    context.attach("smoke.json")?;
    context.receipt.detail =
        json!({"binary_sha256": manifest.files[BINARY], "binary_bytes": bytes, "image": IMAGE});
    Ok(())
}

pub fn review(plan: &Plan, value: &Value) -> Result<()> {
    let keys: std::collections::BTreeSet<_> = value
        .as_object()
        .ok_or("review must be an object")?
        .keys()
        .map(String::as_str)
        .collect();
    if keys
        != [
            "schema_version",
            "commit",
            "case",
            "status",
            "reviewer",
            "evidence",
        ]
        .into()
    {
        return Err("review fields differ from the public review-declaration schema".into());
    }
    if value["schema_version"] != 1
        || value["commit"] != plan.identity.commit
        || value["case"] != "SEC-003"
        || value["status"] != "approved"
        || value["reviewer"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
        || value["evidence"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
        || plan.profile != Profile::Release
    {
        return Err("release requires a commit-specific SEC-003 review from the maintainer-controlled RELEASE_REVIEW variable".into());
    }
    Ok(())
}

pub fn attest(context: &mut Context<'_>, directory: &Path) -> Result<()> {
    let manifest = verify_manifest(context.plan, &context.receipt.plan_sha256, directory)?;
    for name in [BINARY.to_owned(), format!("{BINARY}.sha256")] {
        context.command(vec![
            "gh".into(),
            "attestation".into(),
            "verify".into(),
            directory.join(name).display().to_string(),
            "--repo".into(),
            "Develata/math_talk_radar".into(),
            "--signer-workflow".into(),
            "Develata/math_talk_radar/.github/workflows/release.yml".into(),
            "--source-digest".into(),
            context.plan.identity.commit.clone(),
            "--deny-self-hosted-runners".into(),
            "--format".into(),
            "json".into(),
        ])?;
    }
    context.receipt.detail = json!({"binary_sha256": manifest.files[BINARY]});
    Ok(())
}
