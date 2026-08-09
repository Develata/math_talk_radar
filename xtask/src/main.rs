//! Project-local dev tooling (§58).
//!
//! Commands:
//!   cargo xtask check          — source-registry + acceptance-matrix + doc-coverage validation
//!   cargo xtask check-matrix   — acceptance-matrix structural + doc-coverage validation
//!   cargo xtask baseline       — functional/quality/perf baseline orchestration (M7/M8)
//!   cargo xtask static-release <binary> — musl/static-link checks (M7)
//!
//! M0 ships `check` and `check-matrix`; M7 ships `baseline` and
//! `static-release`.
#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = workspace_root();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("check");
    let result = match cmd {
        "check" => run_check(&root),
        "check-matrix" => run_check_matrix(&root),
        "baseline" => run_baseline(&root),
        "static-release" => {
            let binary = args.get(1).map(Path::new);
            match binary {
                Some(p) => run_static_release(p),
                None => Err(vec!["usage: cargo xtask static-release <binary>".into()]),
            }
        }
        other => {
            eprintln!("unknown xtask command: {other}");
            eprintln!("available: check | check-matrix | baseline | static-release");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => {
            println!("xtask {cmd}: ok");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("error: {e}");
            }
            eprintln!("xtask {cmd}: FAILED ({} error(s))", errors.len());
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Compile-time invariant: xtask lives directly under the workspace root.
    manifest_dir
        .parent()
        .expect("xtask must live directly under the workspace root")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// static-release: verify a release binary is statically linked (RELS-001, §51)
// ---------------------------------------------------------------------------

fn run_static_release(binary: &Path) -> Result<(), Vec<String>> {
    use std::process::Command;

    let mut errors: Vec<String> = Vec::new();

    if !binary.exists() {
        return Err(vec![format!("binary not found: {}", binary.display())]);
    }

    let file_out = Command::new("file")
        .arg(binary)
        .output()
        .map_err(|e| vec![format!("failed to run `file`: {e}")])?;
    let file_text = String::from_utf8_lossy(&file_out.stdout);
    println!("file: {file_text}");

    let statically_linked = file_text.contains("statically linked");
    if !statically_linked {
        errors.push(format!(
            "RELS-001: `file` does not report 'statically linked'.\n\
             Output: {file_text}"
        ));
    }

    let ldd = Command::new("ldd").arg(binary).output();
    match ldd {
        Ok(out) => {
            let ldd_text = String::from_utf8_lossy(&out.stdout);
            let ldd_err = String::from_utf8_lossy(&out.stderr);
            if out.status.success() {
                println!("ldd: {ldd_text}");
                let has_deps = ldd_text
                    .lines()
                    .any(|l| !l.trim().is_empty() && !l.contains("not a dynamic executable"));
                if has_deps {
                    errors.push(format!(
                        "RELS-001: `ldd` reports runtime shared-library dependencies.\n\
                         Output: {ldd_text}"
                    ));
                }
            } else {
                let combined = format!("{ldd_text}{ldd_err}");
                let not_dynamic = combined.contains("not a dynamic executable");
                println!("ldd: {combined}");
                if !not_dynamic {
                    errors.push(format!(
                        "RELS-001: `ldd` failed without 'not a dynamic executable'.\n\
                         Output: {combined}"
                    ));
                }
            }
        }
        Err(e) => {
            errors.push(format!("failed to run `ldd`: {e}"));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// baseline: functional + quality + perf orchestration (M7/M8, §57 B5)
// ---------------------------------------------------------------------------

fn run_baseline(root: &Path) -> Result<(), Vec<String>> {
    use std::process::Command;

    let mut errors: Vec<String> = Vec::new();

    println!("baseline: functional (cargo test --workspace)");
    let test = Command::new("cargo")
        .args(["test", "--workspace"])
        .current_dir(root)
        .status();
    match test {
        Ok(s) if s.success() => {}
        Ok(s) => errors.push(format!("functional: cargo test failed ({s})")),
        Err(e) => errors.push(format!("functional: failed to run cargo test: {e}")),
    }

    println!("baseline: quality (fmt + clippy)");
    let fmt = Command::new("cargo")
        .args(["fmt", "--check"])
        .current_dir(root)
        .status();
    match fmt {
        Ok(s) if s.success() => {}
        Ok(s) => errors.push(format!("quality: cargo fmt --check failed ({s})")),
        Err(e) => errors.push(format!("quality: failed to run cargo fmt: {e}")),
    }
    let clippy = Command::new("cargo")
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(root)
        .status();
    match clippy {
        Ok(s) if s.success() => {}
        Ok(s) => errors.push(format!("quality: cargo clippy failed ({s})")),
        Err(e) => errors.push(format!("quality: failed to run cargo clippy: {e}")),
    }

    println!("baseline: perf (RSS adapter memory, PERF-001 ≤128 MiB)");
    let perf = Command::new("cargo")
        .args([
            "run",
            "-p",
            "radar-adapters",
            "--example",
            "perf_rss",
            "--release",
        ])
        .current_dir(root)
        .output();
    match perf {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let peak_kb: Option<u64> = stdout
                .lines()
                .find_map(|l| l.strip_prefix("PERF_RSS_PEAK_KB:"))
                .and_then(|v| v.trim().parse().ok());
            let events: Option<u64> = stdout
                .lines()
                .find_map(|l| l.strip_prefix("PERF_RSS_EVENTS:"))
                .and_then(|v| v.trim().parse().ok());
            match (peak_kb, events) {
                (Some(kb), Some(ev)) => {
                    let mib = kb as f64 / 1024.0;
                    println!("baseline: perf peak RSS = {kb} KiB ({mib:.1} MiB), {ev} events");
                    let limit_kb: u64 = 128 * 1024;
                    if kb > limit_kb {
                        errors.push(format!(
                            "PERF-001: peak RSS {kb} KiB exceeds 128 MiB ({limit_kb} KiB)"
                        ));
                    }
                }
                _ => errors.push(format!("perf: failed to parse perf_rss output:\n{stdout}")),
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            errors.push(format!("perf: perf_rss exited non-zero\n{stderr}"));
        }
        Err(e) => errors.push(format!("perf: failed to run perf_rss: {e}")),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// check: source-registry + acceptance-matrix + doc coverage
// ---------------------------------------------------------------------------

fn run_check(root: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    errors.extend(validate_source_registry(root));
    errors.extend(validate_matrix(root));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn run_check_matrix(root: &Path) -> Result<(), Vec<String>> {
    let errors = validate_matrix(root);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// source-registry validation (§17)
// ---------------------------------------------------------------------------

const SRC_COLS: &[&str] = &[
    "id",
    "name",
    "tier",
    "kind",
    "adapter",
    "entrypoint",
    "allowed_hosts",
    "max_depth",
    "request_budget",
    "media_strategy",
    "dynamic",
    "enabled",
    "fixture",
    "last_verified",
    "status",
    "notes",
];
const SRC_REQUIRED: &[&str] = &[
    "id",
    "name",
    "tier",
    "kind",
    "adapter",
    "max_depth",
    "request_budget",
    "dynamic",
    "enabled",
    "status",
];
const VALID_TIERS: &[&str] = &["S", "A", "B", "unknown"];
const VALID_KINDS: &[&str] = &[
    "institution_calendar",
    "conference_series",
    "rss_feed",
    "ics_feed",
    "indico",
    "jsonld",
    "media_archive",
    "other",
];
const VALID_ADAPTERS: &[&str] = &[
    "rss",
    "ics",
    "jsonld",
    "indico",
    "html_config",
    "html_generic",
    "none",
];
const VALID_SRC_STATUS: &[&str] = &[
    "pending_audit",
    "audited",
    "enabled",
    "disabled",
    "broken",
    "dynamic_unsupported",
];

fn validate_source_registry(root: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    let path = root.join("docs/registry/source-registry.tsv");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return vec![format!("cannot read {}: {e}", path.display())],
    };
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return vec!["source-registry.tsv is empty".into()];
    }
    let header: Vec<&str> = lines[0].split('\t').collect();
    for col in SRC_COLS {
        if !header.contains(col) {
            errors.push(format!("source-registry: missing required column '{col}'"));
        }
    }
    let idx = |name: &str| header.iter().position(|h| *h == name);
    let i_id = idx("id");
    let i_tier = idx("tier");
    let i_kind = idx("kind");
    let i_adapter = idx("adapter");
    let i_depth = idx("max_depth");
    let i_budget = idx("request_budget");
    let i_dyn = idx("dynamic");
    let i_en = idx("enabled");
    let i_status = idx("status");

    let i_fixture = idx("fixture");

    let mut seen = HashSet::new();
    let mut audited_count: usize = 0;
    let mut enabled_fixture_count: usize = 0;
    let mut pending_audit_count: usize = 0;
    let mut enabled_adapter_kinds: HashSet<&str> = HashSet::new();
    for (i, line) in lines.iter().enumerate().skip(1) {
        let row: Vec<&str> = line.split('\t').collect();
        let cell = |ri: Option<usize>| ri.and_then(|x| row.get(x)).copied().unwrap_or("");
        let id = cell(i_id);
        for col in SRC_REQUIRED {
            let ci = idx(col);
            if cell(ci).is_empty() {
                errors.push(format!(
                    "source-registry row {i} ({id}): empty required column '{col}'"
                ));
            }
        }
        if !seen.insert(id.to_string()) {
            errors.push(format!("source-registry row {i}: duplicate id '{id}'"));
        }
        if !VALID_TIERS.contains(&cell(i_tier)) {
            errors.push(format!(
                "source-registry row {i} ({id}): invalid tier '{}'",
                cell(i_tier)
            ));
        }
        if !VALID_KINDS.contains(&cell(i_kind)) {
            errors.push(format!(
                "source-registry row {i} ({id}): invalid kind '{}'",
                cell(i_kind)
            ));
        }
        if !VALID_ADAPTERS.contains(&cell(i_adapter)) {
            errors.push(format!(
                "source-registry row {i} ({id}): invalid adapter '{}'",
                cell(i_adapter)
            ));
        }
        if !cell(i_dyn).is_empty() && !["true", "false"].contains(&cell(i_dyn)) {
            errors.push(format!(
                "source-registry row {i} ({id}): dynamic must be true/false"
            ));
        }
        if !cell(i_en).is_empty() && !["true", "false"].contains(&cell(i_en)) {
            errors.push(format!(
                "source-registry row {i} ({id}): enabled must be true/false"
            ));
        }
        for (col, val) in [
            ("max_depth", cell(i_depth)),
            ("request_budget", cell(i_budget)),
        ] {
            if !val.is_empty() && val.parse::<u32>().is_err() {
                errors.push(format!(
                    "source-registry row {i} ({id}): {col} not an integer: '{val}'"
                ));
            }
        }
        let status = cell(i_status);
        if !VALID_SRC_STATUS.contains(&status) {
            errors.push(format!(
                "source-registry row {i} ({id}): invalid status '{}'",
                status
            ));
        }

        if status == "pending_audit" {
            pending_audit_count += 1;
        } else {
            audited_count += 1;
        }
        if cell(i_en) == "true" {
            let fixture = cell(i_fixture);
            if !fixture.is_empty() {
                enabled_fixture_count += 1;
            }
            enabled_adapter_kinds.insert(cell(i_adapter));
        }
    }

    // LIVE-001/002 coverage baseline (§18). Only enforced once the audit is
    // complete — while any row is still pending_audit, the counts are not
    // checked so the gate doesn't fail during an in-progress audit.
    if pending_audit_count == 0 {
        if audited_count < 20 {
            errors.push(format!(
                "LIVE-001: need >=20 audited sources, got {audited_count}"
            ));
        }
        if enabled_fixture_count < 10 {
            errors.push(format!(
                "LIVE-002: need >=10 enabled fixture-backed sources, got {enabled_fixture_count}"
            ));
        }
        if enabled_adapter_kinds.len() < 3 {
            errors.push(format!(
                "coverage: need >=3 distinct adapter kinds among enabled sources, got {} ({})",
                enabled_adapter_kinds.len(),
                enabled_adapter_kinds
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// acceptance-matrix validation (§55, §56, DOC-001, DOC-002)
// ---------------------------------------------------------------------------

const MATRIX_COLS: &[&str] = &[
    "case_id",
    "requirement",
    "plan_ref",
    "test_surface",
    "automation",
    "gate",
    "evidence",
    "status",
];
const MATRIX_REQUIRED: &[&str] = &[
    "case_id",
    "requirement",
    "plan_ref",
    "test_surface",
    "automation",
    "gate",
    "status",
];
const VALID_GATES: &[&str] = &["hard", "advisory"];
const VALID_STATUS: &[&str] = &["pending", "pass", "fail", "skipped"];

fn validate_matrix(root: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    let path = root.join("docs/registry/acceptance-matrix.tsv");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return vec![format!("cannot read {}: {e}", path.display())],
    };
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return vec!["acceptance-matrix.tsv is empty".into()];
    }
    let header: Vec<&str> = lines[0].split('\t').collect();
    for col in MATRIX_COLS {
        if !header.contains(col) {
            errors.push(format!(
                "acceptance-matrix: missing required column '{col}'"
            ));
        }
    }
    let idx = |name: &str| header.iter().position(|h| *h == name);
    let i_case = idx("case_id");
    let i_plan = idx("plan_ref");
    let i_ts = idx("test_surface");
    let i_auto = idx("automation");
    let i_gate = idx("gate");
    let i_status = idx("status");
    let i_ev = idx("evidence");

    let mut seen = HashSet::new();
    let mut referenced_plans: HashSet<String> = HashSet::new();
    for (i, line) in lines.iter().enumerate().skip(1) {
        let row: Vec<&str> = line.split('\t').collect();
        let cell = |ri: Option<usize>| ri.and_then(|x| row.get(x)).copied().unwrap_or("");
        let case_id = cell(i_case);
        let plan_ref = cell(i_plan);
        let test_surface = cell(i_ts);
        let automation = cell(i_auto);
        let gate = cell(i_gate);
        let status = cell(i_status);
        let evidence = cell(i_ev);

        for col in MATRIX_REQUIRED {
            let ci = idx(col);
            if cell(ci).is_empty() {
                errors.push(format!(
                    "matrix row {i} ({case_id}): empty required column '{col}'"
                ));
            }
        }
        if !seen.insert(case_id.to_string()) {
            errors.push(format!("matrix row {i}: duplicate case_id '{case_id}'"));
        }
        if !VALID_GATES.contains(&gate) {
            errors.push(format!("matrix row {i} ({case_id}): invalid gate '{gate}'"));
        }
        if !VALID_STATUS.contains(&status) {
            errors.push(format!(
                "matrix row {i} ({case_id}): invalid status '{status}'"
            ));
        }
        if gate == "hard" {
            if test_surface.is_empty() {
                errors.push(format!(
                    "matrix row {i} ({case_id}): hard gate with empty test_surface"
                ));
            }
            if automation.is_empty() {
                errors.push(format!(
                    "matrix row {i} ({case_id}): hard gate with empty automation (DOC-002)"
                ));
            }
        }
        if !plan_ref.is_empty() {
            let p = root.join(plan_ref);
            if !p.exists() {
                errors.push(format!(
                    "matrix row {i} ({case_id}): plan_ref not found: {plan_ref}"
                ));
            } else {
                referenced_plans.insert(plan_ref.to_string());
            }
        }
        if !evidence.is_empty() {
            let p = root.join(evidence);
            if !p.exists() {
                errors.push(format!(
                    "matrix row {i} ({case_id}): evidence not found: {evidence}"
                ));
            }
        }
    }

    // DOC-001: every plan file must be referenced by ≥1 acceptance case.
    let plan_dir = root.join("docs/plan");
    if let Ok(rd) = std::fs::read_dir(&plan_dir) {
        let mut all_plans: HashSet<String> = HashSet::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("md")
                && let Ok(rel) = p.strip_prefix(root)
            {
                all_plans.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        for plan in &all_plans {
            if !referenced_plans.contains(plan) {
                errors.push(format!(
                    "DOC-001: plan '{plan}' has no acceptance case referencing it"
                ));
            }
        }
    }

    errors
}
