# Acceptance Matrix (human-readable)

> Machine-readable source of truth: `docs/registry/acceptance-matrix.tsv`.
> This file summarizes it. `cargo xtask check-matrix` validates structural
> completeness (unique IDs, plan refs exist, hard cases have test_surface +
> automation, every plan is referenced — DOC-001/DOC-002).

## Summary

- 69 cases total.
- 68 `hard` gate, 1 `advisory` (LIVE-003).
- 54 `pass` (M1: DATE-001..005, PER-001..003, TOP-001, RANK-001..003; M2:
  SRC-001..008, MED-001..003, HTTP-001..003, REL-001..002, TALK-001; M3:
  DEDUP-001..002, REL-003, STATE-001..004; M4: CLI-001..004, CFG-001..002,
  HTTP-004..005; M5: UPD-001..004, UNS-001..004; M6: LIVE-001..002),
  15 `pending`.

## By group

| Group | Cases | Count |
|---|---|---|
| CLI | CLI-001..004 | 4 |
| CFG | CFG-001..002 | 2 |
| DATE | DATE-001..005 | 5 |
| SRC | SRC-001..008 | 8 |
| HTTP | HTTP-001..005 | 5 |
| PER | PER-001..003 | 3 |
| TOP | TOP-001 | 1 |
| TALK | TALK-001 | 1 |
| MED | MED-001..003 | 3 |
| DEDUP | DEDUP-001..002 | 2 |
| RANK | RANK-001..003 | 3 |
| STATE | STATE-001..004 | 4 |
| UPD | UPD-001..004 | 4 |
| UNS | UNS-001..004 | 4 |
| SEC | SEC-001..003 | 3 |
| PERF | PERF-001..002 | 2 |
| REL | REL-001..003 | 3 |
| DOC | DOC-001..002 | 2 |
| RELS | RELS-001..003 | 3 |
| LIVE | LIVE-001..003 | 3 |
| **Total** | | **69** |

Per-case detail: `docs/acceptance-cases/`.
