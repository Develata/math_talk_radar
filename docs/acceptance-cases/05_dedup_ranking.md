# 05 — Dedup & Ranking

| ID | Requirement | Gate | Status |
|---|---|---|---|
| DEDUP-001 | identical event merge | hard | pending |
| DEDUP-002 | distinct events not merged | hard | pending |
| RANK-001 | topic score component | hard | pending |
| RANK-002 | recording score component | hard | pending |
| RANK-003 | title-only scholar mention gives no people boost | hard | pending |

Plan ref: `docs/plan/06_normalization_matching.md` (DEDUP),
`docs/plan/08_ranking.md` (RANK). Golden datasets: dedup pairs ≥ 30, ranking
cases ≥ 20. Conservative dedup precision = 100% on labeled baseline; a wrong
merge is a release blocker (§47). Lands in M1 (ranking primitives) / M3 (dedup).
