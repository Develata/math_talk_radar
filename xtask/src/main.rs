//! Project-local dev tooling (§58).
//!
//! Commands:
//!   cargo xtask check          — source-registry + acceptance-matrix + doc-coverage validation
//!   cargo xtask check-matrix   — acceptance-matrix structural + doc-coverage validation
//!   cargo xtask baseline       — functional/quality/perf baseline orchestration (M7/M8)
//!   cargo xtask static-release <binary> — musl/static-link checks (M7)
//!   cargo xtask acceptance plan|run|summarize — run-bound acceptance evidence
//!
//! M0 ships `check` and `check-matrix`; M7 ships `baseline` and
//! `static-release`.
#![forbid(unsafe_code)]

mod acceptance;
mod architecture;
mod perf;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// Fixture HTTP must stay on loopback even when the shell has HTTP proxies.
// Apply only to offline test/probe children, never to the product or live scans.
const FIXTURE_NO_PROXY: [(&str, &str); 2] = [
    ("NO_PROXY", "localhost,127.0.0.1,::1"),
    ("no_proxy", "localhost,127.0.0.1,::1"),
];

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "artifact-smoke") {
        let result = if args.len() == 4 {
            acceptance::smoke::run(Path::new(&args[1]), &args[2], Path::new(&args[3]))
        } else {
            Err("usage: artifact-smoke <binary> <sha256> <output.json>".into())
        };
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let root = match workspace_root(&mut args) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let cmd = args.first().map(String::as_str).unwrap_or("check");
    let result = match cmd {
        "check" => run_check(&root),
        "check-matrix" => run_check_matrix(&root),
        "baseline" => run_baseline(&root),
        "perf" => match &args[1..] {
            [] => perf::run(&root),
            [option, path] if option == "--out" => perf::run_to(&root, Path::new(path)),
            _ => Err(vec!["usage: perf [--out <report.json>]".into()]),
        },
        "acceptance" => acceptance::cli(&root, &args[1..]).map_err(|error| vec![error]),
        "static-release" => {
            let binary = args.get(1).map(Path::new);
            match binary {
                Some(p) => run_static_release(p),
                None => Err(vec!["usage: cargo xtask static-release <binary>".into()]),
            }
        }
        other => {
            eprintln!("unknown xtask command: {other}");
            eprintln!(
                "available: check | check-matrix | baseline | perf | static-release | acceptance | artifact-smoke"
            );
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

fn workspace_root(args: &mut Vec<String>) -> Result<PathBuf, String> {
    let explicit = args.first().is_some_and(|arg| arg == "--workspace-root");
    let start = if explicit {
        if args.len() < 2 {
            return Err("--workspace-root requires a directory".into());
        }
        let path = PathBuf::from(&args[1]);
        args.drain(..2);
        path
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?
    };
    let start = start.canonicalize().map_err(|e| e.to_string())?;
    for candidate in start.ancestors() {
        if candidate.join("xtask/Cargo.toml").is_file()
            && candidate
                .join("docs/plan/00_engineering_constitution.md")
                .is_file()
        {
            return Ok(candidate.to_owned());
        }
        if explicit {
            break;
        }
    }
    Err("workspace not found; use --workspace-root <checkout>".into())
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
        .env("LC_ALL", "C")
        .arg("--brief")
        .arg(binary)
        .output()
        .map_err(|e| vec![format!("failed to run `file`: {e}")])?;
    let file_text = String::from_utf8_lossy(&file_out.stdout);
    println!("file: {file_text}");

    let statically_linked = file_out.status.success() && static_file_description(&file_text);
    if !statically_linked {
        errors.push(format!(
            "RELS-001: `file` does not report a static ELF executable.\n\
             Output: {file_text}"
        ));
    }

    let ldd = Command::new("ldd").env("LC_ALL", "C").arg(binary).output();
    match ldd {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            println!("ldd: {combined}");
            if !static_ldd_output(out.status.success(), &combined) {
                errors.push(format!(
                    "RELS-001: `ldd` did not confirm absence of shared-library dependencies.\n\
                     Output: {combined}"
                ));
            }
        }
        Err(e) => errors.push(format!("failed to run `ldd`: {e}")),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// `file --brief` omits the path so a filename cannot impersonate linkage.
fn static_file_description(text: &str) -> bool {
    text.starts_with("ELF ")
        && (text.contains(", statically linked,") || text.contains(", static-pie linked,"))
}

fn static_ldd_output(success: bool, text: &str) -> bool {
    // Static PIE has a dynamic section for self-relocation, but no loader or
    // DT_NEEDED libraries. ldd reports it successfully as "statically linked".
    match text.trim() {
        "statically linked" => success,
        "not a dynamic executable" => true,
        _ => false,
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
        .envs(FIXTURE_NO_PROXY)
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

    if let Err(meta_errors) = run_check(root) {
        errors.extend(meta_errors);
    }
    if let Err(perf_errors) = perf::run(root) {
        errors.extend(perf_errors);
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
    errors.extend(validate_schema_drift(root));
    errors.extend(architecture::validate(root));
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
// schema drift validation (§09:38, H4)
// ---------------------------------------------------------------------------

/// H4: verify the golden JSON Schema file exists. The actual drift check
/// (regenerate schema from Rust model, compare to golden) runs as a Rust
/// integration test in `apps/cli/tests/schema_drift.rs` so it can call
/// `schemars::schema_for!` in-process. This xtask gate only verifies the
/// golden file is present and non-empty — catching accidental deletion
/// without duplicating the build logic here.
fn validate_schema_drift(root: &Path) -> Vec<String> {
    let golden = root.join("docs/reference/output-schema.json");
    let mut errors = Vec::new();
    match std::fs::read_to_string(&golden) {
        Ok(content) => {
            if content.trim().is_empty() {
                errors.push(format!(
                    "schema drift: {} is empty (regenerate with `cargo run -- schema > {}`)",
                    golden.display(),
                    golden.display()
                ));
            }
            if !content.contains("\"ScanOutput\"") {
                errors.push(format!(
                    "schema drift: {} does not contain the ScanOutput root schema (regenerate with `cargo run -- schema > {}`)",
                    golden.display(),
                    golden.display()
                ));
            }
        }
        Err(e) => {
            errors.push(format!(
                "schema drift: cannot read {}: {e} (regenerate with `cargo run -- schema > {}`)",
                golden.display(),
                golden.display()
            ));
        }
    }
    errors
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
    "list_fixture",
    "detail_fixture",
    "last_verified",
    "status",
    "notes",
];
const ADAPTERS_WITH_DETAIL: &[&str] = &["rss", "ics", "jsonld", "html_config", "html_generic"];
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
    let i_list_fixture = idx("list_fixture");
    let i_detail_fixture = idx("detail_fixture");
    let i_media = idx("media_strategy");

    let mut seen = HashSet::new();
    let mut audited_count: usize = 0;
    let mut enabled_fixture_count: usize = 0;
    let mut pending_audit_count: usize = 0;
    let mut media_source_count: usize = 0;
    let mut enabled_adapter_kinds: HashSet<&str> = HashSet::new();
    for (i, line) in lines.iter().enumerate().skip(1) {
        let row: Vec<&str> = line.split('\t').collect();
        let cell = |ri: Option<usize>| ri.and_then(|x| row.get(x)).copied().unwrap_or("");
        let id = cell(i_id);
        for col in SRC_REQUIRED {
            if cell(idx(col)).is_empty() {
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
                "source-registry row {i} ({id}): invalid status '{status}'"
            ));
        }

        if status == "pending_audit" {
            pending_audit_count += 1;
        } else {
            audited_count += 1;
        }
        if cell(i_en) == "true" {
            let fixture = cell(i_list_fixture);
            if !fixture.is_empty() {
                let fixture_path = root
                    .join("crates/radar-adapters/tests/fixtures")
                    .join(fixture);
                if fixture_path.exists() {
                    enabled_fixture_count += 1;
                } else {
                    errors.push(format!(
                        "source-registry row {i} ({id}): list_fixture '{fixture}' not found on disk"
                    ));
                }
            }
            let detail = cell(i_detail_fixture);
            if !detail.is_empty() {
                let detail_path = root
                    .join("crates/radar-adapters/tests/fixtures")
                    .join(detail);
                if !detail_path.exists() {
                    errors.push(format!(
                        "source-registry row {i} ({id}): detail_fixture '{detail}' not found on disk"
                    ));
                }
            } else if ADAPTERS_WITH_DETAIL.contains(&cell(i_adapter))
                && cell(i_media) != "youtube_channel"
            {
                errors.push(format!(
                    "source-registry row {i} ({id}): enabled source with adapter '{}' requires detail_fixture (§45)",
                    cell(i_adapter)
                ));
            }
            enabled_adapter_kinds.insert(cell(i_adapter));

            let test_fn_prefix = format!("fn site_{}", id.replace('-', "_"));
            let audits_path = root.join("crates/radar-adapters/tests/site_audits.rs");
            match std::fs::read_to_string(&audits_path) {
                Ok(audits) if !audits.contains(&test_fn_prefix) => errors.push(format!(
                    "source-registry row {i} ({id}): no golden test in site_audits.rs (expected function starting with '{test_fn_prefix}')"
                )),
                Err(e) => errors.push(format!("source-registry: cannot read site_audits.rs: {e}")),
                _ => {}
            }
        }

        if cell(i_en) == "true" {
            let kind = cell(i_kind);
            let media_strategy = cell(i_media);
            let valid_strategies = ["youtube_channel"];
            if !media_strategy.is_empty() {
                if !valid_strategies.contains(&media_strategy) {
                    errors.push(format!(
                        "source-registry row {i} ({id}): unsupported v0.1 media_strategy '{media_strategy}', expected youtube_channel"
                    ));
                }
                media_source_count += 1;
            } else if kind.contains("recording")
                || kind.contains("media")
                || kind.contains("video")
                || kind.contains("archive")
            {
                media_source_count += 1;
            }
        }
    }

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
        if enabled_adapter_kinds.len() < 2 {
            errors.push(format!(
                "coverage: need >=2 distinct adapter kinds among enabled sources, got {} ({})",
                enabled_adapter_kinds.len(),
                enabled_adapter_kinds
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if media_source_count < 3 {
            eprintln!(
                "warning: §18 coverage: need >=3 media/recording sources, got {media_source_count}"
            );
        }
    }
    errors
}

fn validate_matrix(root: &Path) -> Vec<String> {
    acceptance::catalog::validate(root)
}

#[cfg(test)]
mod static_linkage_tests {
    use super::*;

    #[test]
    fn accepts_static_elf_and_static_pie() {
        for text in [
            "ELF 64-bit LSB executable, x86-64, statically linked, stripped",
            "ELF 64-bit LSB pie executable, x86-64, static-pie linked, stripped",
        ] {
            assert!(static_file_description(text));
        }
        assert!(static_ldd_output(true, "\tstatically linked\n"));
        assert!(static_ldd_output(false, "\tnot a dynamic executable\n"));
    }

    #[test]
    fn rejects_dynamic_non_elf_and_ambiguous_tool_output() {
        for text in [
            "ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped",
            "ASCII text, statically linked, something",
            "statically linked: ELF 64-bit LSB executable, dynamically linked, stripped",
            "",
        ] {
            assert!(!static_file_description(text));
        }
        for text in [
            "linux-vdso.so.1 (0x123)\nlibc.so.6 => /lib/libc.so.6 (0x456)",
            "statically linked\nlibc.so.6 => /lib/libc.so.6 (0x456)",
            "ldd: not found",
            "",
        ] {
            assert!(!static_ldd_output(true, text));
            assert!(!static_ldd_output(false, text));
        }
        assert!(!static_ldd_output(false, "statically linked"));
    }
}
