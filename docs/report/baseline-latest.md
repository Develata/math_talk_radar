# Baseline — Latest

> Evidence only — non-authoritative. Populated by `cargo xtask baseline` (§57).
> Most recent run: 2026-08-21, at HEAD (pre-v0.1.0-tag), toolchain rustc 1.97.1.

## B1 Functional

| Suite | Result (2026-08-21) |
|---|---|
| `cargo test -p radar-core` | 157 passed |
| `cargo test -p radar-fetch` | 44 passed |
| `cargo test -p radar-adapters` | 194 passed |
| `cargo test -p radar-state` | 50 passed |
| `cargo test -p math_talk_radar` (integration + lifecycle + schema) | 70 passed |
| `cargo test --workspace` | 519 passed, 0 failed |
| Date parser accuracy (§47) | 1.000 (57/57) |
| Scholar precision (§47) | 1.000 |
| Scholar recall (§47) | 1.000 |
| Role-protection FP (§47) | 0 |

## B2 Quality

| Check | Result (2026-08-21) |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 519 passed |
| `cargo xtask check` | ok |
| `cargo xtask check-matrix` | ok |
| `forbid(unsafe_code)` | enforced in every crate |
| Acceptance cases `pass` | 65/65 (0 pending) |

## B3 Performance

| Metric | Result (2026-08-13) |
|---|---|
| RSS adapter peak memory (PERF-001) | 6736 KiB (6.6 MiB), 4000 events |
| Budget (§13) | 128 MiB |
| Margin | 19.5× under budget |

## B4 Release

| Check | Result (2026-08-21) |
|---|---|
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo xtask static-release <b>` | not run locally (CI gate on release.yml) |
| release.yml wired | quality gates → musl build → container smoke → SHA-256 → attestation |
| MSRV 1.96 check | CI job (ci.yml + release.yml) |
| Coverage gates | radar-core ≥85%, workspace ≥75% (CI job on release.yml) |

## B5 Live Source

| Metric | Result (2026-08-21) |
|---|---|
| Sources audited | 27 |
| Sources enabled + fixture-backed | 16 (5 RSS, 11 HTML-config) |
| `pending_audit` rows | 0 |
| Adapter kinds among enabled | 2 (rss, html_config) |
| `cargo xtask live-smoke` | implemented (R3-P1-01); scheduled in live-smoke.yml |
