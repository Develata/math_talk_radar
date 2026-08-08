# Implementation Status

> Evidence only — non-authoritative (§0.2). Updated per milestone.

## Current milestone: M0 — Repository Bootstrap

- [x] Rust 2024 workspace: `radar-core`, `radar-fetch`, `radar-adapters`,
      `radar-state`, `apps/cli`, `xtask`.
- [x] Crate DAG enforced; `#![forbid(unsafe_code)]` in every crate.
- [x] `cargo check --workspace --all-targets` passes.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [x] `cargo fmt --check` clean.
- [x] `cargo xtask check` and `cargo xtask check-matrix` pass.
- [x] Root + nested `AGENTS.md`; `CLAUDE.md`; 14 plan skeletons; acceptance
      matrix (69 cases) + source registry (24 candidates).
- [x] 4 ADRs; 4 reference docs; roadmap; report stubs; CI skeletons.

## Next: M1 — Core Domain

Date parser, normalization pipeline, scholar/topic matching, ranking primitives,
golden datasets (dates ≥ 50, people ≥ 60, dedup ≥ 30, ranking ≥ 20).
