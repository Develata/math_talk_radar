# AGENTS.md — `docs/`

## Authority

- `docs/plan/` is the **engineering design truth source**. Code is a projection.
- `docs/acceptance-cases/` are the **verifiable proof** of the plans.
- `docs/registry/` holds machine-readable matrices validated by `cargo xtask`.
- `docs/adr/` records architecture decisions; it must not silently override the
  plan. If a decision changes the design, update the plan too.
- `docs/reference/` is reference material (CLI, schemas).
- `docs/report/` records evidence only — **non-authoritative**.

## Synchronization rules

- The acceptance matrix (`docs/registry/acceptance-matrix.tsv`) and the
  human-readable `docs/acceptance-matrix.md` must stay synchronized.
- Every plan file in `docs/plan/` must be referenced by ≥1 acceptance case
  (DOC-001, enforced by `cargo xtask check-matrix`).
- Every hard-gate case must declare a `test_surface` and `automation`
  (DOC-002, enforced by `cargo xtask check-matrix`).
- When a plan changes, update the linked acceptance cases in the same commit.
- Reports (`docs/report/`) are append-only evidence; never edit to mask a
  failure — supersede with a newer entry.
