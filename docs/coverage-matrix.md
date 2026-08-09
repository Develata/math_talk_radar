# Coverage Matrix

> Maps plan requirements to test surfaces. The authoritative machine-readable
> view is `docs/registry/acceptance-matrix.tsv`; this file is the human summary.
> Populated as cases move from `pending` to `pass`.

## Per-milestone coverage (target)

| Milestone | Cases landing | Primary surfaces |
|---|---|---|
| M1 | DATE-001..005, PER-001..003, TOP-001, RANK-001..003 | unit, golden |
| M2 | SRC-001..008, MED-001..003, HTTP-001..003, REL-001..002, TALK-001 | fixture, mock server |
| M3 | DEDUP-001..002, STATE-001..004, REL-003 | golden, integration |
| M4 | CLI-001..004, CFG-001..002, HTTP-004..005 | integration |
| M5 | UPD-001..004, UNS-001..004 | sandbox |
| M6 | LIVE-001..002 | registry, fixture |
| M7 | SEC-002, PERF-001..002, RELS-001..003 | CI, baseline, release |
| M8 | SEC-001, SEC-003, DOC-001..002 | lint, review, xtask |

## Code coverage targets (§54)

- `radar-core` line coverage ≥ 85%.
- Workspace meaningful code ≥ 75%.
- Adapter fixture coverage emphasized; schema glue not padded for numbers.

Coverage tool: `cargo llvm-cov` (M7).
