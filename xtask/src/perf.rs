//! Small offline performance gate. Timings are trends; only broad catastrophic
//! limits and relatively stable resource properties fail CI.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime};

fn cargo(root: &Path, args: &[&str]) -> Result<String, Vec<String>> {
    println!("perf: cargo {}", args.join(" "));
    let output = Command::new("cargo")
        .args(args)
        .envs(crate::FIXTURE_NO_PROXY)
        .current_dir(root)
        .output()
        .map_err(|e| vec![format!("cargo {}: {e}", args.join(" "))])?;
    if !output.status.success() {
        return Err(vec![format!(
            "cargo {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )]);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn startup_ms(binary: &Path, flag: &str) -> Result<f64, Vec<String>> {
    let mut samples = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let result = Command::new(binary)
            .arg(flag)
            .stdout(Stdio::null())
            .status()
            .map_err(|e| vec![format!("startup {flag}: {e}")])?;
        if !result.success() {
            return Err(vec![format!("startup {flag}: {result}")]);
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    Ok((samples[14] + samples[15]) / 2.0)
}

pub(super) fn run(root: &Path) -> Result<(), Vec<String>> {
    let metadata = super::architecture::metadata(root).map_err(|e| vec![e])?;
    let target = metadata["target_directory"]
        .as_str()
        .ok_or_else(|| vec!["Cargo target_directory missing".into()])?;
    let output_path = Path::new(target).join("perf-latest.json");
    run_to(root, &output_path)
}

pub(super) fn run_to(root: &Path, output_path: &Path) -> Result<(), Vec<String>> {
    // Replace stale success before any build/probe can fail. Acceptance supplies
    // a run-owned path so concurrent invocations cannot exchange reports.
    write_report(
        output_path,
        &json!({"schema_version": 1, "status": "running"}),
    )?;
    let started = Instant::now();
    let result = run_probes(root);
    let report = match &result {
        Ok(report) => report.clone(),
        Err(errors) => json!({"schema_version": 1, "status": "fail", "errors": errors}),
    };
    let mut report = report;
    report["duration_ms"] = json!(started.elapsed().as_millis());
    write_report(output_path, &report)?;
    println!("perf: metrics {}", output_path.display());
    result.map(|_| ())
}

fn write_report(path: &Path, report: &Value) -> Result<(), Vec<String>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| vec![e.to_string()])?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(report).map_err(|e| vec![e.to_string()])?,
    )
    .and_then(|()| std::fs::rename(temporary, path))
    .map_err(|e| vec![e.to_string()])
}

fn built_executable(messages: &str) -> Result<PathBuf, Vec<String>> {
    let mut executables = Vec::new();
    for line in messages.lines() {
        let message: Value = serde_json::from_str(line).map_err(|e| vec![e.to_string()])?;
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == "math_talk_radar"
            && let Some(path) = message["executable"].as_str()
        {
            executables.push(PathBuf::from(path));
        }
    }
    match executables.as_slice() {
        [executable] => Ok(executable.clone()),
        _ => Err(vec![
            "build must identify exactly one math_talk_radar executable".into(),
        ]),
    }
}

fn run_probes(root: &Path) -> Result<Value, Vec<String>> {
    let build_messages = cargo(
        root,
        &[
            "build",
            "--offline",
            "--locked",
            "--release",
            "-p",
            "math_talk_radar",
            "--message-format=json",
        ],
    )?;
    let binary = built_executable(&build_messages)?;
    let binary_sha256 = crate::acceptance::evidence::file_hash(&binary).map_err(|e| vec![e])?;
    let size = std::fs::metadata(&binary)
        .map_err(|e| vec![e.to_string()])?
        .len();
    let help_ms = startup_ms(&binary, "--help")?;
    let version_ms = startup_ms(&binary, "--version")?;
    let workload: Value = serde_json::from_str(&cargo(
        root,
        &[
            "run",
            "--offline",
            "--locked",
            "--release",
            "-p",
            "math_talk_radar",
            "--example",
            "perf_pipeline",
        ],
    )?)
    .map_err(|e| vec![format!("pipeline metrics: {e}")])?;
    let scan: Value = serde_json::from_str(&cargo(
        root,
        &[
            "run",
            "--offline",
            "--locked",
            "--release",
            "-p",
            "math_talk_radar",
            "--example",
            "perf_scan",
        ],
    )?)
    .map_err(|e| vec![format!("scan metrics: {e}")])?;
    let storage_pipeline = cargo(
        root,
        &[
            "run",
            "--offline",
            "--locked",
            "--release",
            "-p",
            "math_talk_radar",
            "--example",
            "perf_storage_pipeline",
            "--",
            binary
                .to_str()
                .ok_or_else(|| vec!["non-UTF-8 performance binary path".into()])?,
        ],
    )?;
    for required in [
        "PERF_PIPELINE_CASE:n=1000;",
        "PERF_PIPELINE_CASE:n=5000;",
        "PERF_PIPELINE_CASE:n=10000;",
        "PERF_PIPELINE_HIGH_COLLISION:",
        "PERF_PIPELINE_BINARY_BYTES:",
        "PERF_PIPELINE_STARTUP_MS:",
        "PERF_PIPELINE_PEAK_KB:",
    ] {
        if !storage_pipeline
            .lines()
            .any(|line| line.starts_with(required) && !line.contains("unavailable"))
        {
            return Err(vec![format!("storage pipeline metric missing: {required}")]);
        }
    }
    let storage_rss_kb = storage_pipeline
        .lines()
        .find_map(|line| line.strip_prefix("PERF_PIPELINE_PEAK_KB:"))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .ok_or_else(|| vec!["storage pipeline RSS metric invalid".into()])?;
    let rss = cargo(
        root,
        &[
            "run",
            "--offline",
            "--locked",
            "--release",
            "-p",
            "radar-adapters",
            "--example",
            "perf_rss",
        ],
    )?;
    let rss_kb = rss
        .lines()
        .find_map(|l| l.strip_prefix("PERF_RSS_PEAK_KB:"))
        .and_then(|s| s.trim().parse::<u64>().ok())
        .ok_or_else(|| vec!["adapter RSS metric missing".into()])?;
    let mut errors = Vec::new();
    for (name, value, limit) in [
        ("binary_bytes", Some(size as f64), 30.0 * 1024.0 * 1024.0),
        ("startup_help_ms", Some(help_ms), 1000.0),
        ("startup_version_ms", Some(version_ms), 1000.0),
        ("adapter_rss_kb", Some(rss_kb as f64), 128.0 * 1024.0),
        (
            "pipeline_rss_kb",
            workload["peak_rss_kb"].as_f64(),
            128.0 * 1024.0,
        ),
        ("scan_rss_kb", scan["peak_rss_kb"].as_f64(), 128.0 * 1024.0),
        (
            "storage_pipeline_rss_kb",
            Some(storage_rss_kb as f64),
            128.0 * 1024.0,
        ),
        (
            "dedup_10000_ms",
            workload["dedup_ms"]["10000"].as_f64(),
            5000.0,
        ),
        (
            "dedup_collision_10000_ms",
            workload["dedup_ms"]["collision_10000"].as_f64(),
            5000.0,
        ),
    ] {
        match value {
            Some(value) if value.is_finite() && value > 0.0 && value <= limit => {}
            _ => errors.push(format!(
                "performance gate {name}: {value:?}, allowed (0, {limit}]"
            )),
        }
    }
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    let worktree_status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let rustc = Command::new("rustc")
        .arg("-vV")
        .current_dir(root)
        .output()
        .map_err(|e| vec![e.to_string()])?;
    if !rustc.status.success() {
        return Err(vec!["rustc -vV failed".into()]);
    }
    let report = json!({"schema_version": 1, "status": if errors.is_empty() { "pass" } else { "fail" },
        "revision": revision, "worktree_status": worktree_status,
        "toolchain": String::from_utf8_lossy(&rustc.stdout), "profile": "release",
        "binary": binary, "binary_sha256": binary_sha256,
        "generated_at_unix_seconds": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|e| vec![e.to_string()])?.as_secs(),
        "binary_bytes": size, "startup_help_ms": help_ms, "startup_version_ms": version_ms,
        "adapter_rss_kb": rss_kb, "workload": workload, "scan": scan, "storage_pipeline": storage_pipeline, "hard_gate_errors": errors});
    println!(
        "perf: binary {size} bytes; help {help_ms:.2} ms; version {version_ms:.2} ms; adapter RSS {rss_kb} KiB"
    );
    if errors.is_empty() {
        Ok(report)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_run_invalidates_only_its_owned_report() {
        let directory = tempfile::tempdir().unwrap();
        let report = directory.path().join("run/performance.json");
        let other = directory.path().join("other/performance.json");
        write_report(&report, &json!({"status": "pass"})).unwrap();
        write_report(&other, &json!({"status": "pass"})).unwrap();
        // No Cargo manifest: build fails before any expensive probes run.
        assert!(run_to(directory.path(), &report).is_err());
        let status =
            |path| -> Value { serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap() };
        assert_eq!(status(report)["status"], "fail");
        assert_eq!(status(other)["status"], "pass");
    }

    #[test]
    fn build_path_comes_from_cargo_not_native_target_assumptions() {
        let messages = concat!(
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"dep\"},\"executable\":null}\n",
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"math_talk_radar\"},\"executable\":\"/custom/x86_64-unknown-linux-musl/release/math_talk_radar\"}\n",
            "{\"reason\":\"build-finished\",\"success\":true}\n"
        );
        assert_eq!(
            built_executable(messages).unwrap(),
            PathBuf::from("/custom/x86_64-unknown-linux-musl/release/math_talk_radar")
        );
        assert!(built_executable("").is_err());
        assert!(built_executable(&format!("{messages}{messages}")).is_err());
    }
}
