# 04 — People & Topics

| ID | Requirement | Gate |
| --- | --- | --- |
| PER-001 | scholar alias match | hard |
| PER-002 | multilingual alias match | hard |
| PER-003 | concept name not promoted to speaker | hard |
| TOP-001 | topic alias matching | hard |
| TALK-001 | talk + speaker extraction | hard |

Plan ref: `docs/plan/06_normalization_matching.md` (PER, TOP),
`docs/plan/02_domain_model.md` (TALK). Golden datasets (§46): person/entity
cases ≥ 60. Scholar matching precision ≥ 95%, recall ≥ 95%; role-protection
false positives = 0 (§47). Lands in M1 (matching) / M2 (talk extraction).
