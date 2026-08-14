# ADR-0009 — Defer `sources check [id]` to post-v0.1

- Status: Accepted
- Date: 2026-08-15
- Decider: Deve
- Supersedes: none

## Context

Plan §09 (`docs/plan/09_cli_output_contract.md`) lists the CLI surface as:

    `scan`, `sources` (`list` | `check [id]`), `doctor`, `update`, `uninstall`, `schema`.

`sources check [id]` is currently a `not_implemented` stub. The runbook
(`docs/runbook.md:32-33`) shows usage examples (`sources check`,
`sources check clay`), and the CLI reference (`docs/reference/cli.md:12`)
documents the syntax. No acceptance case in
`docs/registry/acceptance-matrix.tsv` gates on `sources check`.

A round-7 external audit flagged this as a plan-vs-code contract gap (H5).

## Decision

Defer `sources check [id]` to post-v0.1. Update plan §09 to mark it as
deferred with a reference to this ADR. The `not_implemented` stub remains
and returns a clear error message (exit 2, "not implemented").

## Rationale

1. **No acceptance case.** The 65 acceptance cases in the matrix do not
   include a `sources check` gate. The v0.1 release contract is defined by
   the acceptance matrix, not by the CLI surface list alone.

2. **Diagnostic, not core.** `sources check` is a convenience diagnostic
   for checking a single source's connectivity and parse health. The core
   scan pipeline (`scan`), source listing (`sources list`), health check
   (`doctor`), and lifecycle (`update`/`uninstall`) are all implemented
   and gated.

3. **Implementation cost is non-trivial.** A proper `sources check` needs
   to wire the fetch engine + adapter pipeline for a single source,
   handle timeout/budget/robots, and report results — essentially a
   single-source dry-run. Implementing this under pre-release audit
   pressure risks introducing bugs in the fetch/adapter path.

4. **The stub is honest.** `not_implemented` returns exit 2 with a clear
   message. It does not silently no-op or claim success.

## Revisit trigger

Implement `sources check [id]` when **any** of the following holds:

- A user or operator requests it for debugging a specific source, or
- v0.2 planning begins, or
- An acceptance case is added that gates on it.

Preferred implementation: reuse `scan_engine`'s fetch+adapter pipeline
with a single-source filter, report event count + fetch/parse errors,
exit 0 on success / exit 4 on zero events / exit 2 on unknown source id.

## Consequences

- Plan §09 updated to note the deferral and reference this ADR.
- `sources check` remains a `not_implemented` stub returning exit 2.
- The runbook and CLI reference should note the deferral (future doc
  update).
- This ADR is the record of the deferral; the review item is resolved by
   documenting the trade-off rather than changing code.
