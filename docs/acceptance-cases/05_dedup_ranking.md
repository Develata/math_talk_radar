# 05 — Dedup & Ranking

| ID | Requirement | Gate |
| --- | --- | --- |
| DEDUP-001 | identical event merge | hard |
| DEDUP-002 | distinct events not merged | hard |
| RANK-001 | topic score component | hard |
| RANK-002 | recording score component | hard |
| RANK-003 | title-only scholar mention gives no people boost | hard |

Plan ref: `docs/plan/06_normalization_matching.md` (DEDUP),
`docs/plan/08_ranking.md` (RANK). Golden datasets: dedup pairs ≥ 30, ranking
cases ≥ 20. Conservative dedup precision = 100% on labeled baseline; a wrong
merge is a release blocker (§47). Lands in M1 (ranking primitives) / M3 (dedup).

DEDUP-001 / RANK-001 regression invariants: opposite interest weights over
identical enriched multi-source candidates must preserve every non-ranking
field. Equal IDs from different sources must select the same representative
under input permutation. Covered by core dedup unit tests.

DEDUP-001/002: indexed output equals the test-only linear reference on 1200
seeded mixed-key corpora, including absent fields, multiple native IDs, same-ID
ties and changing organizer keys. Explicit bridge tests preserve first-cluster
semantics. Deterministic counters gate linear key lookups for 1k/5k/10k distinct
inputs and bounded key updates for a 10k growing-provenance cluster.

RANK-001/STATE-002: scoring occurs only after persistence reconciliation; legacy
stored scores remain readable and cannot influence first_seen or change records.

DEDUP-002: an empty native ID is missing evidence; it must not merge unrelated
URLs/titles. Both pairwise matching and indexed candidate matching reject it.
