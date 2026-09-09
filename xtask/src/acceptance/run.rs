use super::evidence::{self, CommandEvidence, Result};
use super::junit::{self, TestId};
use super::plan::{self, Plan};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Component, Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: u32,
    pub plan_sha256: String,
    pub run_id: String,
    pub attempt: String,
    pub shard: String,
    pub status: String,
    pub error: Option<String>,
    pub commands: Vec<CommandEvidence>,
    pub attachments: BTreeMap<String, String>,
    pub tests: Vec<TestId>,
    pub cases: Vec<String>,
    pub detail: Value,
}

pub struct Context<'a> {
    pub root: &'a Path,
    pub directory: &'a Path,
    pub plan: &'a Plan,
    pub receipt: Receipt,
}

impl Context<'_> {
    pub fn command(&mut self, argv: Vec<String>) -> Result<String> {
        let environment =
            if ["tests", "coverage", "performance"].contains(&self.receipt.shard.as_str()) {
                crate::FIXTURE_NO_PROXY
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
                    .collect()
            } else {
                BTreeMap::new()
            };
        let evidence = evidence::execute_with_env(
            self.root,
            self.directory,
            self.receipt.commands.len(),
            &argv,
            &environment,
        )?;
        let code = evidence.exit_code;
        let output = evidence.stdout.clone();
        self.receipt.commands.push(evidence);
        if code != Some(0) {
            return Err(format!(
                "command exited {code:?}: {argv:?}; see command logs"
            ));
        }
        Ok(output)
    }

    pub fn attach(&mut self, name: &str) -> Result<()> {
        self.receipt.attachments.insert(
            name.into(),
            evidence::file_hash(&self.directory.join(name))?,
        );
        Ok(())
    }

    pub fn cargo_packages(&self) -> Vec<String> {
        if self.plan.full {
            vec!["--workspace".into()]
        } else {
            self.plan
                .modules
                .iter()
                .flat_map(|module| ["-p".into(), module.clone()])
                .collect()
        }
    }

    fn tests(&mut self) -> Result<()> {
        let config = self.directory.join("nextest.toml");
        let junit = self.directory.join("junit.xml");
        // Each shard gets a new directory; an interrupted prior run cannot leave
        // a reusable JUnit success at this path.
        std::fs::write(&config, format!(
            "[profile.acceptance]\nretries = 0\ntest-threads = 4\ndefault-filter = \"all()\"\nslow-timeout = {{ period = \"30s\", terminate-after = 2 }}\n[profile.acceptance.junit]\npath = {}\nreport-skipped = \"all\"\n",
            serde_json::to_string(&junit.display().to_string()).map_err(|e| e.to_string())?
        )).map_err(|e| e.to_string())?;
        let mut args = vec![
            "cargo".into(),
            "nextest".into(),
            "list".into(),
            "--offline".into(),
            "--locked".into(),
            "--profile".into(),
            "acceptance".into(),
            "--config-file".into(),
            config.display().to_string(),
            "--user-config-file".into(),
            "none".into(),
        ];
        args.extend(self.cargo_packages());
        let mut listing = args.clone();
        listing.push("--message-format=json".into());
        let output = self.command(listing)?;
        let inventory: Value = evidence::read_json(&self.directory.join(&output))?;
        let expected = junit::inventory(&inventory, &self.plan.modules)?;
        for case in plan::selected_cases(self.plan)
            .into_iter()
            .filter(|case| case.check == "tests")
        {
            if !expected.contains(&TestId {
                binary: case.binary.clone(),
                test: case.test.clone(),
            }) {
                return Err(format!("{} maps to a missing exact test ID", case.id));
            }
        }
        evidence::write_json(&self.directory.join("inventory.json"), &inventory)?;
        self.attach("inventory.json")?;
        args[2] = "run".into();
        args.extend(["--no-fail-fast".into(), "--no-tests=fail".into()]);
        let execution = self.command(args);
        let results = junit::results(&std::fs::read_to_string(junit).map_err(|e| e.to_string())?)?;
        self.receipt.tests = expected.into_iter().collect();
        self.attach("junit.xml")?;
        self.attach("nextest.toml")?;
        let failures_outside_selection: Vec<_> = results
            .iter()
            .filter(|(id, status)| {
                *status != "pass"
                    && inventory["rust-suites"][&id.binary]["package-name"]
                        .as_str()
                        .is_some_and(|module| !self.plan.shadow_modules.contains(module))
            })
            .map(|(id, _)| id)
            .collect();
        self.receipt.detail = json!({"test_count": self.receipt.tests.len(), "shadow_failures_outside_selection": failures_outside_selection});
        execution?;
        junit::verify(&self.receipt.tests.iter().cloned().collect(), &results)?;
        // nextest does not execute doctests. Keep the Cargo surface explicitly.
        let mut docs = vec![
            "cargo".into(),
            "test".into(),
            "--offline".into(),
            "--locked".into(),
            "--doc".into(),
        ];
        docs.extend(self.cargo_packages());
        docs.extend(["--".into(), "--include-ignored".into()]);
        self.command(docs)?;
        self.receipt.detail["doctests"] =
            json!("cargo test --doc completed; zero doctests makes no case pass claim");
        Ok(())
    }

    pub fn standard(&mut self, shard: &str) -> Result<()> {
        match shard {
            "meta" => {
                self.command(vec![
                    std::env::current_exe()
                        .map_err(|e| e.to_string())?
                        .display()
                        .to_string(),
                    "--workspace-root".into(),
                    self.root.display().to_string(),
                    "check".into(),
                ])?;
            }
            "quality" => {
                self.command(
                    [
                        "python3",
                        "-B",
                        "-m",
                        "unittest",
                        "discover",
                        "-s",
                        "scripts/tests",
                    ]
                    .map(str::to_owned)
                    .into(),
                )?;
                self.command(["cargo", "fmt", "--check"].map(str::to_owned).into())?;
                let mut args: Vec<_> = [
                    "cargo",
                    "clippy",
                    "--offline",
                    "--locked",
                    "--all-targets",
                    "--all-features",
                ]
                .map(str::to_owned)
                .into();
                if self.plan.modules.is_empty() {
                    args.extend(["-p".into(), "xtask".into()]);
                } else {
                    args.extend(self.cargo_packages());
                }
                args.extend(["--".into(), "-D".into(), "warnings".into()]);
                self.command(args)?;
            }
            "tests" => self.tests()?,
            "security" => {
                self.command(["cargo", "deny", "check"].map(str::to_owned).into())?;
            }
            "coverage" => {
                self.command(vec![
                    "cargo".into(),
                    "llvm-cov".into(),
                    "--offline".into(),
                    "--locked".into(),
                    "--workspace".into(),
                    "--all-features".into(),
                    "--json".into(),
                    "--output-path".into(),
                    self.directory.join("coverage.json").display().to_string(),
                ])?;
                let report: Value = evidence::read_json(&self.directory.join("coverage.json"))?;
                let totals = &report["data"][0]["totals"];
                if totals["lines"]["count"]
                    .as_u64()
                    .is_none_or(|count| count == 0)
                {
                    return Err("coverage has no measured lines".into());
                }
                self.receipt.detail = totals.clone();
                self.attach("coverage.json")?;
            }
            "performance" => {
                let path = self.directory.join("performance.json");
                self.command(vec![
                    std::env::current_exe()
                        .map_err(|e| e.to_string())?
                        .display()
                        .to_string(),
                    "--workspace-root".into(),
                    self.root.display().to_string(),
                    "perf".into(),
                    "--out".into(),
                    path.display().to_string(),
                ])?;
                let report: Value = evidence::read_json(&path)?;
                if report["status"] != "pass" || report["revision"] != self.plan.identity.commit {
                    return Err("performance result is not a current success".into());
                }
                let binary = Path::new(
                    report["binary"]
                        .as_str()
                        .ok_or("performance binary missing")?,
                );
                if report["binary_sha256"] != evidence::file_hash(binary)? {
                    return Err("performance binary changed".into());
                }
                self.attach("performance.json")?;
            }
            "msrv" => {
                self.command(vec![
                    "cargo".into(),
                    format!("+{}", self.plan.msrv),
                    "check".into(),
                    "--offline".into(),
                    "--locked".into(),
                    "--workspace".into(),
                    "--all-targets".into(),
                ])?;
            }
            _ => return Err(format!("unknown standard shard: {shard}")),
        }
        Ok(())
    }
}

pub fn validate_receipt(
    plan: &Plan,
    plan_digest: &str,
    shard: &str,
    directory: &Path,
    receipt: &Receipt,
) -> Result<()> {
    let cases: Vec<_> = plan::selected_cases(plan)
        .into_iter()
        .filter(|case| case.check == shard)
        .map(|case| case.id.clone())
        .collect();
    if receipt.schema_version != 1
        || receipt.plan_sha256 != plan_digest
        || receipt.run_id != plan.run_id
        || receipt.attempt != plan.attempt
        || receipt.shard != shard
        || receipt.status != if shard == "live" { "advisory" } else { "pass" }
        || receipt.error.is_some()
        || receipt.cases != cases
    {
        return Err(format!(
            "{shard}: missing, stale, failed or mismatched receipt"
        ));
    }
    if receipt.commands.is_empty() && shard != "review" {
        return Err(format!("{shard}: no executed commands"));
    }
    validate_commands(plan, receipt)?;
    for (index, command) in receipt.commands.iter().enumerate() {
        if ["tests", "coverage", "performance"].contains(&shard)
            && crate::FIXTURE_NO_PROXY.iter().any(|(key, value)| {
                command.environment.get(*key).map(String::as_str) != Some(*value)
            })
        {
            return Err(format!("{shard}: fixture proxy isolation missing"));
        }
        if command.exit_code != Some(0)
            && !(shard == "live" && index == 0 && command.exit_code == Some(4))
        {
            return Err(format!("{shard}: unsuccessful command"));
        }
        for (name, digest) in [
            (&command.stdout, &command.stdout_sha256),
            (&command.stderr, &command.stderr_sha256),
        ] {
            verify_file(directory, name, digest)?;
        }
    }
    for (name, digest) in &receipt.attachments {
        verify_file(directory, name, digest)?;
    }
    let required: &[&str] = match shard {
        "tests" => &["inventory.json", "junit.xml", "nextest.toml"],
        "coverage" => &["coverage.json"],
        "performance" => &["performance.json"],
        "artifact" => &["manifest.json", "smoke.json"],
        "review" => &["review.json"],
        "build" => &["artifacts/manifest.json"],
        _ => &[],
    };
    if required
        .iter()
        .any(|name| !receipt.attachments.contains_key(*name))
    {
        return Err(format!("{shard}: required attachment missing"));
    }
    if shard == "live" {
        let scan = &receipt.commands[0];
        let health = super::live::classify(
            scan.exit_code,
            &std::fs::read_to_string(directory.join(&scan.stdout)).map_err(|e| e.to_string())?,
        )?;
        let doctor: Value = evidence::read_json(&directory.join(&receipt.commands[1].stdout))?;
        if receipt.detail["health"] != health || doctor["schema_version"] != "1.0" {
            return Err("live evidence does not match actual command output".into());
        }
    }
    if shard == "tests" {
        let inventory: Value = evidence::read_json(&directory.join("inventory.json"))?;
        let expected = junit::inventory(&inventory, &plan.modules)?;
        let actual = junit::results(
            &std::fs::read_to_string(directory.join("junit.xml")).map_err(|e| e.to_string())?,
        )?;
        junit::verify(&expected, &actual)?;
        if receipt
            .tests
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            != expected
            || receipt.tests.len() != expected.len()
        {
            return Err("test receipt inventory mismatch".into());
        }
        for case in plan::selected_cases(plan)
            .into_iter()
            .filter(|case| case.check == "tests")
        {
            if !expected.contains(&TestId {
                binary: case.binary.clone(),
                test: case.test.clone(),
            }) {
                return Err(format!("{} has no executed test", case.id));
            }
        }
    }
    Ok(())
}

fn validate_commands(plan: &Plan, receipt: &Receipt) -> Result<()> {
    let mut commands = Vec::new();
    for command in &receipt.commands {
        let argv: Vec<_> = command.argv.iter().map(String::as_str).collect();
        if !argv.starts_with(&["timeout", "--kill-after=10s", "1200s"]) || argv.len() < 4 {
            return Err("command timeout wrapper missing".into());
        }
        let argv = &argv[3..];
        if argv
            .iter()
            .any(|arg| ["--skip", "--partition", "--filterset", "-E", "-F"].contains(arg))
        {
            return Err("undeclared test filtering in receipt".into());
        }
        commands.push(argv.to_vec());
    }
    let starts = |index: usize, prefix: &[&str]| {
        commands
            .get(index)
            .is_some_and(|args| args.starts_with(prefix))
    };
    let contains = |index: usize, value: &str| {
        commands
            .get(index)
            .is_some_and(|args| args.contains(&value))
    };
    let xtask = |index: usize, operation: &str| {
        commands.get(index).is_some_and(|args| {
            args.len() >= 4 && args[1] == "--workspace-root" && args[3] == operation
        })
    };
    let (count, valid) = match receipt.shard.as_str() {
        "meta" => (1, xtask(0, "check")),
        "quality" => (
            3,
            starts(
                0,
                &[
                    "python3",
                    "-B",
                    "-m",
                    "unittest",
                    "discover",
                    "-s",
                    "scripts/tests",
                ],
            ) && starts(1, &["cargo", "fmt", "--check"])
                && starts(
                    2,
                    &[
                        "cargo",
                        "clippy",
                        "--offline",
                        "--locked",
                        "--all-targets",
                        "--all-features",
                    ],
                )
                && contains(2, "warnings"),
        ),
        "tests" => (
            3,
            starts(0, &["cargo", "nextest", "list"])
                && contains(0, "--message-format=json")
                && starts(1, &["cargo", "nextest", "run"])
                && contains(1, "--no-tests=fail")
                && contains(1, "--no-fail-fast")
                && starts(2, &["cargo", "test", "--offline", "--locked", "--doc"])
                && contains(2, "--include-ignored"),
        ),
        "security" => (1, starts(0, &["cargo", "deny", "check"])),
        "coverage" => (
            1,
            starts(
                0,
                &[
                    "cargo",
                    "llvm-cov",
                    "--offline",
                    "--locked",
                    "--workspace",
                    "--all-features",
                    "--json",
                    "--output-path",
                ],
            ),
        ),
        "performance" => (1, xtask(0, "perf") && contains(0, "--out")),
        "msrv" => (
            1,
            starts(
                0,
                &[
                    "cargo",
                    &format!("+{}", plan.msrv),
                    "check",
                    "--offline",
                    "--locked",
                    "--workspace",
                    "--all-targets",
                ],
            ),
        ),
        "build" => (
            1,
            starts(
                0,
                &[
                    "cargo",
                    "build",
                    "--offline",
                    "--locked",
                    "--release",
                    "--target",
                    super::artifact::TARGET,
                    "-p",
                    "math_talk_radar",
                    "-p",
                    "xtask",
                    "--message-format=json",
                ],
            ),
        ),
        "artifact" => (
            2,
            xtask(0, "static-release")
                && (starts(
                    1,
                    &["docker", "run", "--rm", "--network=none", "--read-only"],
                ))
                && contains(1, "artifact-smoke"),
        ),
        "review" => (0, true),
        "attestation" => (
            2,
            starts(0, &["gh", "attestation", "verify"])
                && starts(1, &["gh", "attestation", "verify"])
                && contains(0, "--source-digest")
                && contains(1, "--source-digest")
                && contains(0, &plan.identity.commit)
                && contains(1, &plan.identity.commit),
        ),
        "live" => (
            2,
            commands.first().is_some_and(|args| {
                args.get(1..) == Some(&["scan", "--no-state", "--format", "json"][..])
            }) && commands
                .get(1)
                .is_some_and(|args| args.get(1..) == Some(&["doctor", "--json"][..])),
        ),
        _ => return Err("unknown command recipe".into()),
    };
    if commands.len() != count || !valid {
        return Err(format!(
            "{}: missing or substituted command recipe",
            receipt.shard
        ));
    }
    if receipt.shard == "tests" {
        for args in &commands {
            let packages: Vec<_> = args
                .windows(2)
                .filter(|pair| pair[0] == "-p")
                .map(|pair| pair[1].to_owned())
                .collect();
            if plan.full {
                if !args.contains(&"--workspace") || !packages.is_empty() {
                    return Err("full test command does not cover workspace".into());
                }
            } else if packages
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                != plan.modules
                || args.contains(&"--workspace")
            {
                return Err("test command does not match selected modules".into());
            }
        }
    }
    Ok(())
}

fn verify_file(directory: &Path, name: &str, digest: &str) -> Result<()> {
    let path = Path::new(name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
        || std::fs::symlink_metadata(directory.join(path))
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        || evidence::file_hash(&directory.join(path))? != digest
    {
        return Err(format!("invalid or changed evidence attachment: {name}"));
    }
    Ok(())
}
