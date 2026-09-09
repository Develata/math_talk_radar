//! Run-local evidence. No cache entry or static status is an execution receipt.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub type Result<T> = std::result::Result<T, String>;

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let size = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if size == 0 {
            return Ok(format!("{:x}", digest.finalize()));
        }
        digest.update(&buffer[..size]);
    }
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("{}: {e}", temporary.display()))?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}

pub fn output(root: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

pub fn now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub commit: String,
    pub source_digest: String,
    pub fixture_digest: String,
    pub lock_digest: String,
    pub toolchain: String,
    pub cargo: String,
    pub flags: BTreeMap<String, String>,
    pub dirty: bool,
}

pub fn identity(root: &Path) -> Result<Identity> {
    let paths = output(
        root,
        "git",
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut paths: Vec<_> = paths.split('\0').filter(|s| !s.is_empty()).collect();
    paths.sort_unstable();
    paths.dedup();
    let mut sources = Vec::new();
    let mut fixtures = Vec::new();
    for relative in paths {
        let path = root.join(relative);
        let digest = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => hash(
                std::fs::read_link(&path)
                    .map_err(|e| e.to_string())?
                    .as_os_str()
                    .as_encoded_bytes(),
            ),
            Ok(metadata) if metadata.is_file() => file_hash(&path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "deleted".into(),
            _ => return Err(format!("unsupported source input: {relative}")),
        };
        let mode = std::fs::symlink_metadata(&path)
            .map(|metadata| metadata.permissions().mode())
            .unwrap_or(0);
        let entry = format!("{relative}\0{mode:o}\0{digest}\0");
        sources.extend_from_slice(entry.as_bytes());
        if relative.contains("fixtures/") || relative.starts_with("config/") {
            fixtures.extend_from_slice(entry.as_bytes());
        }
    }
    let flags = ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET"]
        .into_iter()
        .map(|key| (key.to_owned(), std::env::var(key).unwrap_or_default()))
        .collect();
    Ok(Identity {
        commit: output(root, "git", &["rev-parse", "HEAD"])?
            .trim()
            .to_owned(),
        source_digest: hash(&sources),
        fixture_digest: hash(&fixtures),
        lock_digest: file_hash(&root.join("Cargo.lock"))?,
        toolchain: output(root, "rustc", &["-vV"])?,
        cargo: output(root, "cargo", &["--version"])?,
        flags,
        dirty: !output(root, "git", &["status", "--porcelain"])?
            .trim()
            .is_empty(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEvidence {
    pub argv: Vec<String>,
    pub cwd: String,
    pub environment: BTreeMap<String, String>,
    pub started_at: u64,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
}

#[cfg(test)]
pub fn execute(
    root: &Path,
    directory: &Path,
    number: usize,
    argv: &[String],
) -> Result<CommandEvidence> {
    execute_with_env(root, directory, number, argv, &BTreeMap::new())
}

pub fn execute_with_env(
    root: &Path,
    directory: &Path,
    number: usize,
    argv: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<CommandEvidence> {
    let (program, arguments) = argv.split_first().ok_or("empty command")?;
    let stdout = format!("command-{number}.stdout");
    let stderr = format!("command-{number}.stderr");
    let out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(&stdout))
        .map_err(|e| e.to_string())?;
    let err = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(&stderr))
        .map_err(|e| e.to_string())?;
    let started_at = now()?;
    let started = Instant::now();
    // Linux development tool: coreutils timeout bounds descendants as a group.
    // The actual wrapper argv is recorded, including timeout exit 124/137.
    let status = Command::new("timeout")
        .args(["--kill-after=10s", "1200s", program])
        .args(arguments)
        .envs(environment)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    Ok(CommandEvidence {
        argv: [
            vec!["timeout".into(), "--kill-after=10s".into(), "1200s".into()],
            argv.to_vec(),
        ]
        .concat(),
        cwd: root.display().to_string(),
        environment: environment.clone(),
        started_at,
        duration_ms: u64::try_from(started.elapsed().as_millis()).map_err(|e| e.to_string())?,
        exit_code: status.code(),
        stdout_sha256: file_hash(&directory.join(&stdout))?,
        stderr_sha256: file_hash(&directory.join(&stderr))?,
        stdout,
        stderr,
    })
}
