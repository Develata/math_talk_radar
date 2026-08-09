# 09 — Live Source Smoke

| ID | Requirement | Gate | Status |
|---|---|---|---|
| LIVE-001 | ≥ 20 audited sources in the registry | hard | pass |
| LIVE-002 | ≥ 10 enabled, fixture-backed sources | hard | pass |
| LIVE-003 | live source health ratio (success/total) | advisory | pass |

Plan ref: `docs/plan/01_product_scope.md`. LIVE-001/002 are registry counts
checked by `cargo xtask check`; LIVE-003 is a scheduled live-smoke metric
(`.github/workflows/live-smoke.yml`) and is advisory — a third-party outage must
not fail normal CI (§53, §57 B5). Lands in M6 (audit + fixtures) / M7 (smoke
workflow).
