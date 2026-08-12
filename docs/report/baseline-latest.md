# Baseline — Latest

> Evidence only — non-authoritative. Populated by `cargo xtask baseline` (§57).
> Most recent run: 2026-08-13, after the second-round audit (40 fixes across 9 commits).

## B1 Functional

| Suite | Result (2026-08-13) |
|---|---|
| `cargo test -p radar-core` | 98 passed |
| `cargo test -p radar-fetch` | 25 passed |
| `cargo test -p radar-adapters` | 131 passed |
| `cargo test -p radar-state` | 31 passed |
| `cargo test -p math_talk_radar` (integration) | 8 passed |
| `cargo test -p math_talk_radar` (lifecycle_sandbox) | 9 passed |
| `cargo test --workspace` | 309 passed, 0 failed |
| Date parser accuracy (§47) | 1.000 (57/57) |
| Scholar precision (§47) | 1.000 |
| Scholar recall (§47) | 1.000 |
| Role-protection FP (§47) | 0 |

## B2 Quality

| Check | Result (2026-08-13) |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 309 passed |
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

| Check | Result (2026-08-13) |
|---|---|
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo xtask static-release <b>` | not run locally (CI gate on release.yml) |
| release.yml wired | tag-version check, musl build, static-link verify, size ≤30 MiB, SHA-256, attestation |

## B5 Live Source

| Metric | Result (2026-08-13) |
|---|---|
| Sources audited | 24 |
| Sources enabled + fixture-backed | 15 (2 RSS, 1 JSON-LD, 12 HTML-config) |
| `pending_audit` rows | 0 |
| Adapter kinds among enabled | 3 (rss, json_ld, html_config) |
