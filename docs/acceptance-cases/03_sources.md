# 03 — Source Adapters & Media

| ID | Requirement | Gate | Status |
|---|---|---|---|
| SRC-001 | RSS adapter | hard | pending |
| SRC-002 | ICS adapter | hard | pending |
| SRC-003 | JSON-LD adapter | hard | pending |
| SRC-004 | configured HTML adapter | hard | pending |
| SRC-005 | generic HTML fallback | hard | pending |
| SRC-006 | detail depth ≤ 2 | hard | pending |
| SRC-007 | host allowlist enforced | hard | pending |
| SRC-008 | request budget enforced | hard | pending |
| MED-001 | video detection | hard | pending |
| MED-002 | slides detection | hard | pending |
| MED-003 | public access status | hard | pending |

Plan ref: `docs/plan/04_source_adapter_contract.md`. Fixture-backed (§45);
mock-server cases (SRC-006..008) use a localhost server. Event discovery recall
≥ 95%, media discovery recall ≥ 95% (§47). Lands in M2 (adapters) / M6 (sites).

SRC-008 regression: two sources on one host with content budget 1 both fetch
successfully, regardless of which initializes the shared robots cache. Repeat
with both serial initializers and concurrent admission; one physical robots
request per shared cache. Content redirects/retries still consume budget.

REL-001 / HTTP-001: jobs=3 admits at most three adapters for 60 sources,
including an injected panic, with complete sorted source health. A same-host
cross-port redirect succeeds at per-host concurrency 1; request timeouts are not
extended by a later scan deadline. Skipped requested detail depth is partial.
