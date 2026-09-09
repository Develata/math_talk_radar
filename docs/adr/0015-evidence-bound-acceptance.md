# ADR-0015 — Evidence-bound acceptance and dependency-aware CI

- Status: Accepted for implementation
- Date: 2026-09-09
- Authorization: user approved the preceding CI feasibility audit and requested implementation
- Scope: development and release orchestration; no product or persisted schema change

## Context

CI already has independent jobs, but release only depends on its build job.
The baseline omits coverage and supply-chain checks; the live workflow invokes
unsupported arguments and suppresses errors. Static `pass` matrix rows have no
per-run evidence. Compile-time executable/workspace paths prevent relocation.
428 tests execute in about 11 seconds locally, while raw test binaries total
about 744 MiB. Per-case remote jobs would add substantial transfer/setup cost.

## Decision

Extend the existing Rust xtask, keeping application crate boundaries unchanged.
The acceptance documents own required case IDs; the registry maps them to
executable checks. Plans bind a source snapshot and dependency closure to an
expected check set. Receipts describe actual execution; only the final verifier
can derive overall success. Static documents never assert a current run passed.

Start with whole-crate selection and all reverse consumers, including all Cargo
dependency kinds/targets/features and shared input edges. Unknown changes force
full execution. Incomplete catalogs/graphs are errors, even in full mode. Main,
scheduled, release, deployment/restore and authority/rule changes require full.
Local selection is allowed on the main-only developer checkout; CI on main is
always full. Shadow CI records selection but executes full until measured.

At this stage shared non-Rust inputs have an explicit conservative edge to every
module. Do not attempt lexical whole-program I/O analysis; fixture selection can
be narrowed later only with verified consumer declarations. This keeps the
initial selector useful for private Rust edits without assuming fixture locality.

CI runs a small DAG of checks, test execution, runner and target builds, then
an always-running fail-closed fan-in. Release additionally verifies the exact
musl artifact in clean Ubuntu, checksum read-back and attestation before publish.
Distinct build profiles/toolchains/targets remain distinct artifacts. Cache is
only an optimization. No Docker product, remote test farm, new runtime service,
relaxed self-update trust, branch workflow or automatic publication is added.

Live smoke uses the existing scan command; ADR-0009 remains in force. External
source outages are advisory outcomes, while malformed output/commands are errors.
All processes that can write application state use unique temporary directories.

## Verification and migration

First correct artifact resolution and truthful failure reporting. Then establish
the case catalog, selection plan, execution receipts and rejecting fan-in. Run
selection in shadow alongside full before relying on local selective results.
Test missing cases/shards, unknown paths, dependency consumers, skipped/empty
test selections, stale/wrong-run receipts and wrong artifact checksums. Preserve
fmt, clippy, workspace tests (including doctests), registry/architecture checks,
performance, coverage, cargo-deny and release checks. Review remains required;
an automated security test is not a substitute for a recorded human review.

Rollback changes orchestration back to full execution, never restores static
pass claims or permits release without the full required evidence set. Future
cross-runner partitioning requires a measured benefit over startup/transfer cost.
