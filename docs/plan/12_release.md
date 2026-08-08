# 12 — Release

> Status: M0 skeleton. Authoritative for release engineering. §34.1, §50, §51,
> §52, §53.

## Release profile (§50)

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
strip = "symbols"
```

Modifiable by ADR if benchmarks justify it.

## Static release (§51)

Canonical asset: `x86_64-unknown-linux-musl`. Acceptance: `file` + `ldd` show no
runtime dynamic-library deps; runs on a clean Ubuntu 22.04 container with no
Rust/Cargo/OpenSSL-dev.

## Supply chain (§52)

Workflow: fmt → clippy → test → coverage → cargo-deny → acceptance matrix →
baseline → musl build → static check → smoke → SHA-256 → artifact attestation →
GitHub Release. Minimal permissions; pin third-party actions by full SHA;
Dependabot for Cargo + Actions; release must not skip baseline.

## release.yml (§53)

Trigger: `tag v*`. Must check `tag version == Cargo.toml version`; fail otherwise.

## Assets (§34.1)

`math_talk_radar-x86_64-unknown-linux-musl` + `.sha256` (required); `.tar.gz`,
SBOM (optional).

## Acceptance cases

- PERF-002 — binary ≤30 MiB (release).
- RELS-001 — static musl binary (container).
- RELS-002 — checksum asset (release).
- RELS-003 — artifact provenance (release).
