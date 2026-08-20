# 03 — Source Adapters & Media

| ID | Requirement | Gate | Status |
|---|---|---|---|
| SRC-001 | RSS adapter | hard | pass |
| SRC-002 | ICS adapter | hard | pass |
| SRC-003 | JSON-LD adapter | hard | pass |
| SRC-004 | configured HTML adapter | hard | pass |
| SRC-005 | generic HTML fallback | hard | pass |
| SRC-006 | detail depth ≤ 2 | hard | pass |
| SRC-007 | host allowlist enforced | hard | pass |
| SRC-008 | request budget enforced | hard | pass |
| MED-001 | video detection | hard | pass |
| MED-002 | slides detection | hard | pass |
| MED-003 | public access status | hard | pass |

Plan ref: `docs/plan/04_source_adapter_contract.md`. Fixture-backed (§45);
mock-server cases (SRC-006..008) use a localhost server. Event discovery recall
≥ 95%, media discovery recall ≥ 95% (§47). Lands in M2 (adapters) / M6 (sites).
