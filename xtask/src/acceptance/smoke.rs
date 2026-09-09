//! Standalone, relocatable runner for the exact binary inside clean Ubuntu.
use super::evidence::{self, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

const FEED: &str = include_str!("../../tests/fixtures/smoke-feed.xml");

struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    base: String,
}

impl Server {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let base = format!(
            "http://{}",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        let feed = FEED.replace("{base}", &base);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                        let mut bytes = [0; 8192];
                        let mut size = 0;
                        while size < bytes.len()
                            && !bytes[..size].windows(4).any(|part| part == b"\r\n\r\n")
                        {
                            match stream.read(&mut bytes[size..]) {
                                Ok(0) | Err(_) => break,
                                Ok(read) => size += read,
                            }
                        }
                        let body = if bytes[..size].starts_with(b"GET /feed.xml ") {
                            feed.as_str()
                        } else {
                            ""
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/xml\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            stop,
            thread: Some(thread),
            base,
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn run(binary: &Path, expected_sha: &str, output: &Path) -> Result<()> {
    if evidence::file_hash(binary)? != expected_sha {
        return Err("smoke binary digest mismatch".into());
    }
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let directory = temporary.path();
    let server = Server::start()?;
    let config = format!(
        "[[sources]]\nid = \"artifact-fixture\"\nname = \"Artifact fixture\"\nadapter = \"rss\"\nkind = \"rss_feed\"\nentrypoint = \"{}/feed.xml\"\nenabled = true\nmax_depth = 1\nrequest_budget = 5\n",
        server.base
    );
    let sources = directory.join("sources.toml");
    std::fs::write(&sources, &config).map_err(|e| e.to_string())?;
    let state = directory.join("state.redb");
    let mut environment: BTreeMap<_, _> = ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"]
        .into_iter()
        .map(|key| (key.to_owned(), directory.join(key).display().to_string()))
        .collect();
    environment
        .extend(crate::FIXTURE_NO_PROXY.map(|(key, value)| (key.to_owned(), value.to_owned())));
    let mut commands = Vec::new();
    let mut invoke = |args: Vec<String>| -> Result<String> {
        let argv = [vec![binary.display().to_string()], args].concat();
        let result =
            evidence::execute_with_env(directory, directory, commands.len(), &argv, &environment)?;
        let stdout =
            std::fs::read_to_string(directory.join(&result.stdout)).map_err(|e| e.to_string())?;
        let success = result.exit_code == Some(0);
        commands.push(result);
        if !success {
            return Err("artifact smoke command failed".into());
        }
        Ok(stdout)
    };
    invoke(vec!["--help".into()])?;
    invoke(vec!["--version".into()])?;
    let schema: Value =
        serde_json::from_str(&invoke(vec!["schema".into()])?).map_err(|e| e.to_string())?;
    if !schema.is_object() {
        return Err("schema output is not an object".into());
    }
    let doctor: Value = serde_json::from_str(&invoke(vec!["doctor".into(), "--json".into()])?)
        .map_err(|e| e.to_string())?;
    if doctor["schema_version"] != "1.0" {
        return Err("doctor schema drift".into());
    }
    let scan = vec![
        "scan".into(),
        "--sources".into(),
        sources.display().to_string(),
        "--state".into(),
        state.display().to_string(),
        "--today".into(),
        "2026-09-09".into(),
    ];
    let first: Value = serde_json::from_str(&invoke(scan.clone())?).map_err(|e| e.to_string())?;
    let second: Value = serde_json::from_str(&invoke(scan.clone())?).map_err(|e| e.to_string())?;
    for value in [&first, &second] {
        if value["schema_version"] != "1.0"
            || value["events"]
                .as_array()
                .is_none_or(|events| events.len() != 2)
            || value["source_health"][0]["status"] != "ok"
        {
            return Err("offline artifact scan did not produce two healthy events".into());
        }
    }
    if second["changes"]
        .as_array()
        .is_none_or(|changes| !changes.is_empty())
    {
        return Err("second artifact scan is not unchanged".into());
    }
    let ids = |value: &Value| -> Result<Vec<String>> {
        value["events"]
            .as_array()
            .ok_or("events missing")?
            .iter()
            .map(|event| {
                event["id"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "event ID missing".into())
            })
            .collect()
    };
    if ids(&first)? != ids(&second)? {
        return Err("artifact read-back changed event IDs".into());
    }
    let before = evidence::file_hash(&state)?;
    let mut no_state = scan;
    no_state.push("--no-state".into());
    invoke(no_state)?;
    if evidence::file_hash(&state)? != before {
        return Err("--no-state modified the database".into());
    }
    if evidence::file_hash(binary)? != expected_sha {
        return Err("smoke binary changed during execution".into());
    }
    let report = json!({"schema_version": 1, "status": "pass", "binary_sha256": expected_sha,
        "fixture_sha256": evidence::hash(FEED.as_bytes()), "commands": commands,
        "checks": ["help", "version", "schema", "doctor", "scan", "state-reopen", "no-state"],
        "event_count": 2, "os_release": std::fs::read_to_string("/etc/os-release").map_err(|e| e.to_string())?});
    // Logs are embedded before the sandbox is removed; no dangling log receipts.
    let logs: BTreeMap<_, _> = commands
        .iter()
        .flat_map(|command| [&command.stdout, &command.stderr])
        .map(|name| {
            Ok((
                name.clone(),
                std::fs::read_to_string(directory.join(name)).map_err(|e| e.to_string())?,
            ))
        })
        .collect::<Result<_>>()?;
    let mut report = report;
    report["logs"] = json!(logs);
    evidence::write_json(output, &report)
}
