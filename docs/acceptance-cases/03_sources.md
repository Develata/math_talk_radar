# 03 — Source Adapters & Media

| ID | Requirement | Gate |
| --- | --- | --- |
| SRC-001 | RSS adapter | hard |
| SRC-002 | ICS adapter | hard |
| SRC-003 | JSON-LD adapter | hard |
| SRC-004 | configured HTML adapter | hard |
| SRC-005 | generic HTML fallback | hard |
| SRC-006 | detail depth ≤ 2 | hard |
| SRC-007 | host allowlist enforced | hard |
| SRC-008 | request budget enforced | hard |
| MED-001 | video detection | hard |
| MED-002 | slides detection | hard |
| MED-003 | public access status | hard |

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

SRC-004: runtime selector cache retains at most 256 selectors per thread while
accepting additional valid selectors without caching. Existing source fixtures
cover unchanged media and HTML field extraction after module separation.

SRC-001..005 resource regression: generated 2200-entry RSS, ICS, JSON-LD,
configured HTML and fallback HTML inputs return exactly 2001 stubs. Existing
fetch overflow tests verify 2000 retained candidates and Partial status.

REL-002: after an inline parser reaches the deadline, no subsequent stub is
enriched; the already completed candidate remains and the source is Partial.
