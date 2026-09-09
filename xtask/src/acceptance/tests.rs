use super::*;
use evidence::Identity;
use junit::TestId;

fn identity() -> Identity {
    Identity {
        commit: "a".repeat(40),
        source_digest: "source".into(),
        fixture_digest: "fixtures".into(),
        lock_digest: "lock".into(),
        toolchain: "rustc test".into(),
        cargo: "cargo test".into(),
        flags: BTreeMap::new(),
        dirty: false,
    }
}

fn fixture_plan() -> Plan {
    Plan {
        schema_version: 1,
        identity: identity(),
        runner_sha256: "runner".into(),
        run_id: "run-1".into(),
        attempt: "1".into(),
        profile: Profile::Full,
        full: true,
        base: None,
        changed_paths: Vec::new(),
        reasons: Vec::new(),
        modules: BTreeSet::new(),
        shadow_modules: BTreeSet::new(),
        checks: ["meta".into()].into(),
        cases: Vec::new(),
        catalog_digest: "catalog".into(),
        dependency_graph: BTreeMap::new(),
        msrv: "1.96.0".into(),
    }
}

fn metadata() -> Value {
    json!({"packages": catalog::MODULES.iter().map(|name| json!({"name": name,
        "dependencies": match *name {
            "math_talk_radar" => vec![json!({"name": "radar-fetch", "path": "/fetch"}), json!({"name": "radar-adapters", "path": "/adapters"}), json!({"name": "radar-state", "path": "/state"})],
            "radar-fetch" | "radar-adapters" | "radar-state" => vec![json!({"name": "radar-core", "path": "/core"})],
            _ => Vec::new(),
        }})).collect::<Vec<_>>()})
}

#[test]
fn ci_001_selection_includes_reverse_dev_and_conditional_consumers() {
    let mut metadata = metadata();
    metadata["packages"][4]["dependencies"].as_array_mut().unwrap().push(json!({"name": "radar-fetch", "path": "/fetch", "kind": "dev", "target": "cfg(windows)", "optional": true}));
    let graph = plan::graph(&metadata).unwrap();
    let selected = plan::closure(["radar-fetch".into()].into(), &graph);
    assert_eq!(
        selected,
        [
            "radar-fetch".into(),
            "radar-state".into(),
            "math_talk_radar".into()
        ]
        .into()
    );
    metadata["packages"].as_array_mut().unwrap().pop();
    assert!(plan::graph(&metadata).is_err());
    let (full, selected, _) = plan::select(&["crates/radar-adapters/src/rss.rs".into()], &graph);
    assert!(!full);
    assert_eq!(
        selected,
        ["radar-adapters".into(), "math_talk_radar".into()].into()
    );
}

#[test]
fn ci_002_unknown_and_contract_changes_force_full() {
    let graph = plan::graph(&metadata()).unwrap();
    for path in [
        "new-input/fixture.json",
        "Cargo.lock",
        "rust-toolchain.toml",
        "config/sources.toml",
        "docs/plan/12_release.md",
        "xtask/src/main.rs",
        ".github/workflows/ci.yml",
        "crates/radar-core/src/lib.rs",
        "apps/cli/src/output.rs",
        "crates/radar-adapters/tests/fixtures/rss_feed.xml",
        "crates/radar-core/tests/golden_data/people.toml",
    ] {
        let (full, selected, reasons) = plan::select(&[path.into()], &graph);
        assert!(full, "{path}");
        assert_eq!(selected.len(), catalog::MODULES.len());
        assert!(!reasons.is_empty());
    }
    for profile in [Profile::Full, Profile::Shadow, Profile::Release] {
        let checks = plan::checks(
            &profile,
            true,
            &catalog::MODULES.iter().map(|m| (*m).to_owned()).collect(),
        );
        for check in [
            "tests",
            "coverage",
            "security",
            "performance",
            "msrv",
            "meta",
            "quality",
        ] {
            assert!(checks.contains(check));
        }
    }
}

#[test]
fn ci_003_missing_case_or_shard_mapping_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for path in ["docs/acceptance-cases", "docs/registry", "docs/plan"] {
        std::fs::create_dir_all(root.join(path)).unwrap();
    }
    std::fs::write(root.join("docs/plan/test.md"), "contract").unwrap();
    std::fs::write(
        root.join("docs/acceptance-cases/test.md"),
        "| ID | Requirement | Gate |\n| CI-001 | first | hard |\n| CI-002 | second | hard |\n",
    )
    .unwrap();
    let header =
        "case_id\trequirement\tplan_ref\ttest_surface\tcheck\tmodule\tbinary\ttest\tscope\tgate\n";
    let row = "CI-001\tfirst\tdocs/plan/test.md\tunit\tmeta\t\t\t\tbaseline\thard\n";
    let path = root.join("docs/registry/acceptance-matrix.tsv");
    std::fs::write(&path, format!("{header}{row}")).unwrap();
    assert!(catalog::load(root).is_err());
    std::fs::write(
        &path,
        format!("{header}{row}{}", row.replace("CI-001", "CI-002")),
    )
    .unwrap();
    assert_eq!(catalog::load(root).unwrap().len(), 2);
    std::fs::write(
        &path,
        format!(
            "{header}{row}{}",
            row.replace("CI-001", "CI-002")
                .replace("\tmeta\t", "\tmissing-shard\t")
        ),
    )
    .unwrap();
    assert!(catalog::load(root).is_err());
}

#[test]
fn ci_004_zero_missing_skipped_and_duplicate_tests_are_rejected() {
    let id = TestId {
        binary: "pkg::integration".into(),
        test: "checks_contract".into(),
    };
    let expected = [id.clone()].into();
    let ok = "<testsuites><testsuite><testcase classname=\"pkg::integration\" name=\"checks_contract\"/></testsuite></testsuites>";
    junit::verify(&expected, &junit::results(ok).unwrap()).unwrap();
    let skipped = ok.replace(
        "name=\"checks_contract\"/>",
        "name=\"checks_contract\"><skipped/></testcase>",
    );
    assert!(junit::verify(&expected, &junit::results(&skipped).unwrap()).is_err());
    assert!(
        junit::verify(
            &[
                id,
                TestId {
                    binary: "pkg".into(),
                    test: "missing".into()
                }
            ]
            .into(),
            &junit::results(ok).unwrap()
        )
        .is_err()
    );
    assert!(junit::results("<testsuites/>").is_err());
    assert!(junit::results(&format!("{ok}{ok}")).is_err());
    assert!(
        junit::inventory(
            &json!({"rust-suites": {}, "test-count": 0}),
            &["pkg".into()].into()
        )
        .is_err()
    );
}

#[test]
fn ci_005_fanin_rejects_missing_stale_and_changed_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let plan = fixture_plan();
    assert_eq!(
        summarize(&plan, "plan", directory.path(), "final").unwrap()["status"],
        "fail"
    );
    let shard = directory.path().join("meta");
    std::fs::create_dir(&shard).unwrap();
    let mut command = evidence::execute(directory.path(), &shard, 0, &["true".into()]).unwrap();
    // A synthetic successful meta receipt; this unit fixture is never a release receipt.
    command.argv = [
        "timeout",
        "--kill-after=10s",
        "1200s",
        "fixture-xtask",
        "--workspace-root",
        "/fixture",
        "check",
    ]
    .map(str::to_owned)
    .into();
    let mut receipt = Receipt {
        schema_version: 1,
        plan_sha256: "plan".into(),
        run_id: plan.run_id.clone(),
        attempt: "1".into(),
        shard: "meta".into(),
        status: "pass".into(),
        error: None,
        commands: vec![command],
        attachments: BTreeMap::new(),
        tests: Vec::new(),
        cases: Vec::new(),
        detail: Value::Null,
    };
    write_json(&shard.join("receipt.json"), &receipt).unwrap();
    assert_eq!(
        summarize(&plan, "plan", directory.path(), "final").unwrap()["status"],
        "pass"
    );
    receipt.attempt = "previous".into();
    write_json(&shard.join("receipt.json"), &receipt).unwrap();
    assert_eq!(
        summarize(&plan, "plan", directory.path(), "final").unwrap()["status"],
        "fail"
    );
    receipt.attempt = "1".into();
    receipt.status = "skipped".into();
    assert!(run::validate_receipt(&plan, "plan", "meta", &shard, &receipt).is_err());
    receipt.status = "pass".into();
    std::fs::write(shard.join("command-0.stdout"), "old output replaced").unwrap();
    assert!(run::validate_receipt(&plan, "plan", "meta", &shard, &receipt).is_err());
}

#[test]
fn ci_006_wrong_artifact_digest_and_unreviewed_release_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixture_plan();
    plan.profile = Profile::Release;
    let mut files = BTreeMap::new();
    for name in [artifact::BINARY, artifact::RUNNER] {
        std::fs::write(directory.path().join(name), name).unwrap();
        files.insert(name.to_owned(), evidence::hash(name.as_bytes()));
    }
    let name = format!("{}.sha256", artifact::BINARY);
    std::fs::write(
        directory.path().join(&name),
        format!("{}  {}\n", files[artifact::BINARY], artifact::BINARY),
    )
    .unwrap();
    files.insert(
        name.clone(),
        evidence::file_hash(&directory.path().join(name)).unwrap(),
    );
    let manifest = artifact::Manifest {
        schema_version: 1,
        identity: plan.identity.clone(),
        plan_sha256: "plan".into(),
        target: artifact::TARGET.into(),
        profile: "release".into(),
        features: Vec::new(),
        files,
    };
    write_json(&directory.path().join("manifest.json"), &manifest).unwrap();
    artifact::verify_manifest(&plan, "plan", directory.path()).unwrap();
    assert!(artifact::verify_manifest(&plan, "old-plan", directory.path()).is_err());
    std::fs::write(directory.path().join(artifact::BINARY), "different binary").unwrap();
    assert!(artifact::verify_manifest(&plan, "plan", directory.path()).is_err());
    assert!(artifact::review(&plan, &json!({})).is_err());
    let review = json!({"schema_version": 1, "commit": plan.identity.commit, "case": "SEC-003", "status": "approved", "reviewer": "fixture-maintainer", "evidence": "fixture review"});
    artifact::review(&plan, &review).unwrap();
    let mut wrong = review;
    wrong["commit"] = json!("old-commit");
    assert!(artifact::review(&plan, &wrong).is_err());
}

#[test]
fn live_command_errors_are_not_source_outages() {
    assert!(live::classify(Some(2), "").is_err());
    assert!(live::classify(Some(0), "").is_err());
    assert!(live::classify(Some(0), "{}").is_err());
    assert!(live::classify(None, "").is_err());
    assert_eq!(
        live::classify(Some(4), "").unwrap()["outcome"],
        "unavailable"
    );
    let scan = json!({"schema_version":"1.0", "source_health":[{"source":"ok", "status":"ok"}, {"source":"down", "status":"timeout"}]});
    assert_eq!(
        live::classify(Some(0), &scan.to_string()).unwrap()["health_ratio"],
        0.5
    );
}

fn synthetic_workspace() -> tempfile::TempDir {
    // Entirely synthetic data in a TempDir. Never copy or commit the user's
    // worktree, Git configuration, hooks, credentials or untracked files.
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut members = Vec::new();
    for module in catalog::MODULES {
        let relative = match *module {
            "math_talk_radar" => "apps/cli".to_owned(),
            "xtask" => "xtask".to_owned(),
            _ => format!("crates/{module}"),
        };
        members.push(relative.clone());
        std::fs::create_dir_all(root.join(&relative).join("src")).unwrap();
        std::fs::write(
            root.join(&relative).join("src/lib.rs"),
            "#![forbid(unsafe_code)]\npub fn fixture() {}\n",
        )
        .unwrap();
        std::fs::write(
            root.join(&relative).join("src/worker.rs"),
            "pub fn fixture_worker() {}\n",
        )
        .unwrap();
        let mut manifest = format!(
            "[package]\nname = \"{module}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.96\"\n"
        );
        if ["radar-fetch", "radar-adapters", "radar-state"].contains(module) {
            manifest.push_str("[dependencies]\nradar-core = { path = \"../radar-core\" }\n");
        } else if *module == "math_talk_radar" {
            manifest.push_str("[dependencies]\n");
            for dependency in ["radar-core", "radar-fetch", "radar-adapters", "radar-state"] {
                manifest.push_str(&format!(
                    "{dependency} = {{ path = \"../../crates/{dependency}\" }}\n"
                ));
            }
        }
        std::fs::write(root.join(relative).join("Cargo.toml"), manifest).unwrap();
    }
    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver = \"2\"\nmembers = {}\n",
            serde_json::to_string(&members).unwrap()
        ),
    )
    .unwrap();
    for path in ["docs/plan", "docs/registry", "docs/acceptance-cases"] {
        std::fs::create_dir_all(root.join(path)).unwrap();
    }
    std::fs::write(root.join(".gitignore"), "/target/\n").unwrap();
    std::fs::write(
        root.join("docs/plan/00_engineering_constitution.md"),
        "Synthetic acceptance contract\n",
    )
    .unwrap();
    std::fs::write(
        root.join("docs/acceptance-cases/cases.md"),
        "| ID | Requirement | Gate |\n| CI-001 | fixture | hard |\n",
    )
    .unwrap();
    std::fs::write(root.join("docs/registry/acceptance-matrix.tsv"), "case_id\trequirement\tplan_ref\ttest_surface\tcheck\tmodule\tbinary\ttest\tscope\tgate\nCI-001\tfixture\tdocs/plan/00_engineering_constitution.md\tfixture\tmeta\t\t\t\tbaseline\thard\n").unwrap();
    evidence::output(root, "cargo", &["generate-lockfile", "--offline"]).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "user.name=AcceptanceFixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"]);
    git(&[
        "add",
        "Cargo.toml",
        "Cargo.lock",
        ".gitignore",
        "apps",
        "crates",
        "xtask",
        "docs",
    ]);
    git(&["commit", "--quiet", "-m", "synthetic fixture"]);
    directory
}

#[test]
fn plan_rejects_source_recipe_and_catalog_drift() {
    let workspace = synthetic_workspace();
    let root = workspace.path();
    let plan = plan::create(root, Profile::Full, Some("HEAD".into())).unwrap();
    assert_eq!(plan.msrv, "1.96.0");
    plan::validate(root, &plan).unwrap();
    let mut altered = plan.clone();
    altered.checks.remove("tests");
    assert!(plan::validate(root, &altered).is_err());
    altered = plan.clone();
    altered.cases.clear();
    assert!(plan::validate(root, &altered).is_err());
    std::fs::write(
        root.join("crates/radar-fetch/src/worker.rs"),
        "#![forbid(unsafe_code)]\npub fn changed() {}\n",
    )
    .unwrap();
    assert!(plan::validate(root, &plan).is_err());
    let changed = plan::create(root, Profile::Local, Some("HEAD".into())).unwrap();
    assert_eq!(
        changed.shadow_modules,
        ["radar-fetch".into(), "math_talk_radar".into()].into()
    );
    assert!(
        plan::create(root, Profile::Local, Some("missing-base".into()))
            .unwrap()
            .full
    );
    assert!(plan::create(root, Profile::Local, None).unwrap().full);
    assert!(plan::create(root, Profile::Release, Some("HEAD".into())).is_err());
}

#[test]
fn file_mode_and_symlink_inputs_have_distinct_snapshot_identity() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = synthetic_workspace();
    let root = workspace.path();
    let original = evidence::identity(root).unwrap();
    let file = root.join("crates/radar-fetch/src/lib.rs");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(
        original.source_digest,
        evidence::identity(root).unwrap().source_digest
    );
    std::os::unix::fs::symlink("does-not-exist", root.join("fixture-link")).unwrap();
    // Broken symlinks are hashed as links; external targets are not read.
    let changed = evidence::identity(root).unwrap();
    assert_ne!(original.source_digest, changed.source_digest);
    let plan = plan::create(root, Profile::Local, Some("HEAD".into())).unwrap();
    assert!(plan.full);
}

#[test]
fn smoke_rejects_wrong_digest_and_failing_program() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("fixture-program");
    std::fs::write(&binary, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = directory.path().join("smoke.json");
    assert!(smoke::run(&binary, "wrong", &output).is_err());
    assert!(smoke::run(&binary, &evidence::file_hash(&binary).unwrap(), &output).is_err());
    assert!(!output.exists());
}
#[test]
fn evidence_sha256_matches_known_vector_for_bytes_and_files() {
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(evidence::hash(b"abc"), expected);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abc").unwrap();
    assert_eq!(evidence::file_hash(&path).unwrap(), expected);
}
