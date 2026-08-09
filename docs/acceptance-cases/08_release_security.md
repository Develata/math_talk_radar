# 08 — Release, Security, Reliability

| ID | Requirement | Gate | Status |
|---|---|---|---|
| SEC-001 | no `unsafe` (`forbid(unsafe_code)`) | hard | pass |
| SEC-002 | `cargo deny check` passes | hard | pass |
| SEC-003 | no secret logging | hard | pass |
| PERF-001 | offline RSS scan peak RSS ≤ 128 MiB | hard | pass |
| PERF-002 | release binary ≤ 30 MiB | hard | pass |
| REL-001 | 30% source failure isolation | hard | pass |
| REL-002 | global scan deadline enforced | hard | pass |
| REL-003 | stable deterministic IDs | hard | pass |
| RELS-001 | static musl binary | hard | pass |
| RELS-002 | checksum asset present | hard | pass |
| RELS-003 | artifact provenance attestation | hard | pass |
| DOC-001 | every plan has an acceptance mapping | hard | pass |
| DOC-002 | every hard case has automation | hard | pass |
| HTTP-001 | timeout isolation | hard | pending |
| HTTP-002 | retry transient once | hard | pending |
| HTTP-003 | 404 no retry | hard | pending |

Plan refs: `docs/plan/11_security.md` (SEC), `docs/plan/13_performance_budget.md`
(PERF-001), `docs/plan/12_release.md` (PERF-002, RELS), `docs/plan/05_fetching_reliability.md`
(REL, HTTP-001..003), `docs/plan/00_engineering_constitution.md` (DOC). DOC-001
and DOC-002 are enforced by `cargo xtask check-matrix` (already active in M0).
Lands across M2 (HTTP), M7 (release/deny/coverage), M8 (final).
