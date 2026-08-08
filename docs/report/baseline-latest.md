# Baseline — Latest

> Evidence only — non-authoritative. Populated by `cargo xtask baseline` (§57).
> M0 has no baseline run yet; this file records the most recent results once M1+
> baselines execute.

## B1 Functional

| Suite | M1 result |
|---|---|
| `cargo test -p radar-core` (unit) | 42 passed |
| `cargo test -p radar-core --test golden` | 5 passed (147 golden cases) |
| `cargo test --workspace` | all pass |
| Date parser accuracy (§47) | 1.000 (56/56) |
| Scholar precision (§47) | 1.000 |
| Scholar recall (§47) | 1.000 |
| Role-protection FP (§47) | 0 |

## B2 Quality

| Check | M1 result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace` | 42 unit + 5 golden, all pass |
| `cargo xtask check` | ok |
| `cargo xtask check-matrix` | ok |
| `forbid(unsafe_code)` | enforced in every crate |
| Acceptance cases `pass` | 12 (DATE×5, PER×3, TOP×1, RANK×3) |

## B3 Performance

_(not run until M7)_

## B4 Release

_(not run until M7)_

## B5 Live Source

_(not run until M6/M7; 24 sources in `pending_audit` status)_
