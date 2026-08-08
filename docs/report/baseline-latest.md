# Baseline — Latest

> Evidence only — non-authoritative. Populated by `cargo xtask baseline` (§57).
> M0 has no baseline run yet; this file records the most recent results once M1+
> baselines execute.

## B1 Functional

_(not run until M1)_

## B2 Quality

| Check | M0 result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | 4 passed (normalize×2, retry×2) |
| `cargo xtask check` | ok |
| `cargo xtask check-matrix` | ok |
| `forbid(unsafe_code)` | enforced in every crate |

## B3 Performance

_(not run until M7)_

## B4 Release

_(not run until M7)_

## B5 Live Source

_(not run until M6/M7; 24 sources in `pending_audit` status)_
