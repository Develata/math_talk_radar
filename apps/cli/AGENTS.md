# AGENTS.md — `apps/cli`

## Purpose

Composition root. Thin command layer that wires `radar-core`, `radar-fetch`,
`radar-adapters`, and `radar-state` into the `math_talk_radar` binary. Owns the
user-facing CLI surface, output rendering, and the self-update / uninstall
lifecycle.

## Authority

Follows `docs/plan/09_cli_output_contract.md` and `docs/plan/10_update_uninstall.md`.

## Hard boundaries

- **Composition root.** This is the ONLY crate allowed to depend on all four
  library crates (§11). It must not leak business logic downward.
- **Thin command layer.** Commands parse args, call the pipeline, render output.
  No domain decisions live here.
- **stdout/stderr contract (§28).** `scan` stdout is machine-readable JSON only;
  logs/progress go to stderr. Management commands (`doctor`/`update`/`uninstall`/
  `sources`) may be human-readable by default and offer `--json`.
- **Startup (§48).** `--version` / `--help` must perform no network or state
  initialization. Keep the clap parse path side-effect free.
- **Lifecycle safety (§34, §35).** Self-update verifies SHA-256 before replace,
  keeps a rollback copy, and never deletes the working binary on failure.
  Uninstall deletes only known app-owned paths; never `rm -rf $HOME`.
- **`#![forbid(unsafe_code)]`.**

## Exit codes (§32)

0 success (incl. partial source failure) · 2 usage · 3 config/schema · 4 zero
usable sources · 5 state fatal · 6 output serialization fatal · 10 update ·
11 uninstall.
