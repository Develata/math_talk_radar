# Security Policy

## Scope

This policy covers the `math_talk_radar` CLI and its workspace crates
(`radar-core`, `radar-fetch`, `radar-adapters`, `radar-state`, `apps/cli`).

## Threat model (summary)

- **Untrusted input**: all fetched HTML/JSON/ICS/RSS is treated as untrusted.
  Parsers must not `panic` on malformed input and must never execute embedded
  scripts or follow `javascript:` URLs.
- **Network**: outbound HTTPS only. `robots.txt` and per-source request budgets
  are enforced (§15). rustls — no system OpenSSL.
- **Self-update (§34)**: SHA-256 verification, rollback copy, atomic replace,
  self-test before and after replace. Origin pinned to a single constant.
- **Uninstall (§35)**: deletes only known app-owned paths; never `rm -rf $HOME`;
  never follows arbitrary symlinks; protects `cargo run` development binaries
  unless `--force-unmanaged`.
- **State (§22)**: `redb` is embedded and ACID; state is rebuildable from a scan
  but `first_seen` / media history must not be silently lost.
- **Secrets**: the project uses no API keys, tokens, or credentials for its
  public-source collection. Configuration contains only public URLs and
  preference weights.

## Hard prohibitions (§6)

- `unsafe` is forbidden in every crate (`#![forbid(unsafe_code)]`).
- No network I/O in `radar-core` or `radar-adapters`.
- No `unwrap`/`expect`/`panic!` in production code without a documented
  compile-time invariant.
- No real-website dependency in `cargo test` (§44).

## Reporting a vulnerability

Please report security issues privately by opening a security advisory on the
GitHub repository, or email the maintainer directly. Do not open a public issue
for security vulnerabilities.

## Acceptance cases

See `SEC-001` (no `unsafe`), `SEC-002` (supply-chain via `cargo deny`), and
`SEC-003` (uninstall safety) in `docs/registry/acceptance-matrix.tsv`.
