//! Mechanical enforcement of the workspace DAG in docs/plan/03_architecture.md.
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

pub(super) fn metadata(root: &Path) -> Result<Value, String> {
    let result = Command::new("cargo")
        .args([
            "metadata",
            "--offline",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    serde_json::from_slice(&result.stdout).map_err(|e| e.to_string())
}

pub(super) fn validate(root: &Path) -> Vec<String> {
    let mut errors = match metadata(root) {
        Ok(metadata) => validate_packages(&metadata),
        Err(error) => vec![error],
    };
    // Preserve the former CI forbid-unsafe gate, including both executable roots.
    for relative in [
        "crates/radar-core/src/lib.rs",
        "crates/radar-fetch/src/lib.rs",
        "crates/radar-adapters/src/lib.rs",
        "crates/radar-state/src/lib.rs",
        "apps/cli/src/lib.rs",
        "apps/cli/src/main.rs",
        "xtask/src/main.rs",
    ] {
        match std::fs::read_to_string(root.join(relative)) {
            Ok(source)
                if source
                    .lines()
                    .any(|line| line.trim() == "#![forbid(unsafe_code)]") => {}
            _ => errors.push(format!("missing forbid(unsafe_code): {relative}")),
        }
    }
    errors
}

fn validate_packages(metadata: &Value) -> Vec<String> {
    let Some(packages) = metadata["packages"].as_array() else {
        return vec!["metadata packages missing".into()];
    };
    let internal: HashSet<_> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut errors = Vec::new();
    for package in packages {
        let name = package["name"].as_str().unwrap_or("");
        let (allowed, forbidden): (&[&str], &[&str]) = match name {
            "radar-core" => (&[], &["reqwest", "redb", "scraper", "tokio", "feed-rs"]),
            "radar-fetch" => (&["radar-core"], &["scraper", "feed-rs", "redb"]),
            "radar-adapters" => (&["radar-core"], &["reqwest", "redb", "tokio"]),
            "radar-state" => (&["radar-core"], &["reqwest", "scraper", "feed-rs", "tokio"]),
            "math_talk_radar" => (
                &["radar-core", "radar-fetch", "radar-adapters", "radar-state"],
                &[],
            ),
            "xtask" => (&[], &[]),
            _ => {
                errors.push(format!("architecture: undeclared workspace package {name}"));
                continue;
            }
        };
        let Some(dependencies) = package["dependencies"].as_array() else {
            errors.push(format!("{name}: missing dependencies"));
            continue;
        };
        for dependency in dependencies {
            if dependency["kind"] == "dev" {
                continue;
            }
            let dep = dependency["name"].as_str().unwrap_or("");
            if forbidden.contains(&dep) || (internal.contains(dep) && !allowed.contains(&dep)) {
                errors.push(format!(
                    "architecture: forbidden dependency {name} -> {dep}"
                ));
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_reverse_and_io_dependencies_but_allows_test_fixtures() {
        let metadata = json!({"packages": [
            {"name":"radar-core", "dependencies":[{"name":"radar-fetch"}, {"name":"reqwest"}, {"name":"tokio", "kind":"dev"}]},
            {"name":"radar-fetch", "dependencies":[{"name":"radar-core"}]}
        ]});
        let errors = validate_packages(&metadata);
        assert_eq!(errors.len(), 2);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("radar-core -> radar-fetch"))
        );
        assert!(errors.iter().any(|e| e.contains("radar-core -> reqwest")));
    }
}
