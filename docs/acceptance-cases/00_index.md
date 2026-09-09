# Acceptance Cases — Index

> Case tables in this directory own required IDs and gate strength. The machine-readable
> `docs/registry/acceptance-matrix.tsv` maps each to an executable check and exact
> test ID where applicable. `cargo xtask check-matrix` verifies both sets match.

Static definitions do not assert execution status. Per-run plans and receipts
under `target/acceptance/` distinguish pass, fail, not-selected, not-run and
advisory outcomes. A required skipped/missing check blocks its profile. Baseline
scope does not claim release-only or live cases passed; security review remains
required for release. Historical reports apply only to their recorded revision.

## Gate legend

- `hard` — must pass for v0.1.0 release.
- `advisory` — monitored; not a release gate.

## Files

| File | Cases |
|---|---|
| [01_cli.md](01_cli.md) | CLI-001..004, CFG-001..002, HTTP-004..005 |
| [02_dates.md](02_dates.md) | DATE-001..005 |
| [03_sources.md](03_sources.md) | SRC-001..008, MED-001..003 |
| [04_people_topics.md](04_people_topics.md) | PER-001..003, TOP-001, TALK-001 |
| [05_dedup_ranking.md](05_dedup_ranking.md) | DEDUP-001..002, RANK-001..003 |
| [06_state_changes.md](06_state_changes.md) | STATE-001..004 |
| [07_update_uninstall.md](07_update_uninstall.md) | UPD-001..004, UNS-001..004 |
| [08_release_security.md](08_release_security.md) | SEC-001..003, PERF-001..002, REL-001..003, RELS-001..003, DOC-001..002, HTTP-001..003 |
| [09_live_smoke.md](09_live_smoke.md) | LIVE-001..003 |
| [10_ci.md](10_ci.md) | CI-001..006 |

Case IDs remain stable; current results come only from a verified run summary.
