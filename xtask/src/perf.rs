//! Small offline performance gate. Timings are trends; only broad catastrophic
//! limits and relatively stable resource properties fail CI.
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime};

fn cargo(root: &Path, args: &[&str]) -> Result<String, Vec<String>> {
    println!("perf: cargo {}", args.join(" "));
    let output = Command::new("cargo")
        .args(args)
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
    cargo(
        root,
        &[
            "build",
            "--offline",
            "--locked",
            "--release",
            "-p",
            "math_talk_radar",
        ],
    )?;
    let metadata = super::architecture::metadata(root).map_err(|e| vec![e])?;
    let target = metadata["target_directory"]
        .as_str()
        .ok_or_else(|| vec!["Cargo target_directory missing".into()])?;
    let target = Path::new(target);
    let binary = target.join("release/math_talk_radar");
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
    let report = json!({"schema_version": 1, "revision": revision, "worktree_status": worktree_status,
        "generated_at_unix_seconds": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|e| vec![e.to_string()])?.as_secs(),
        "binary_bytes": size, "startup_help_ms": help_ms, "startup_version_ms": version_ms,
        "adapter_rss_kb": rss_kb, "workload": workload, "scan": scan, "hard_gate_errors": errors});
    let output_path = target.join("perf-latest.json");
    std::fs::write(
        &output_path,
        serde_json::to_vec_pretty(&report).map_err(|e| vec![e.to_string()])?,
    )
    .map_err(|e| vec![e.to_string()])?;
    println!("perf: metrics {}", output_path.display());
    println!(
        "perf: binary {size} bytes; help {help_ms:.2} ms; version {version_ms:.2} ms; adapter RSS {rss_kb} KiB"
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
