# 08 — Release, Security, Reliability

| ID | Requirement | Gate |
| --- | --- | --- |
| SEC-001 | no `unsafe` (`forbid(unsafe_code)`) | hard |
| SEC-002 | `cargo deny check` passes | hard |
| SEC-003 | no secret logging | hard |
| PERF-001 | offline RSS scan peak RSS ≤ 128 MiB | hard |
| PERF-002 | release binary ≤ 30 MiB | hard |
| REL-001 | 30% source failure isolation | hard |
| REL-002 | global scan deadline enforced | hard |
| REL-003 | stable deterministic IDs | hard |
| RELS-001 | static musl binary | hard |
| RELS-002 | checksum asset present | hard |
| RELS-003 | artifact provenance attestation | hard |
| DOC-001 | every plan has an acceptance mapping | hard |
| DOC-002 | every hard case has automation | hard |
| HTTP-001 | timeout isolation | hard |
| HTTP-002 | retry transient once | hard |
| HTTP-003 | 404 no retry | hard |

Plan refs: `docs/plan/11_security.md` (SEC), `docs/plan/13_performance_budget.md`
(PERF-001), `docs/plan/12_release.md` (PERF-002, RELS), `docs/plan/05_fetching_reliability.md`
(REL, HTTP-001..003), `docs/plan/00_engineering_constitution.md` (DOC). DOC-001
and DOC-002 are enforced by `cargo xtask check-matrix` (already active in M0).
Lands across M2 (HTTP), M7 (release/deny/coverage), M8 (final).

2026-09-09 additions: `cargo xtask check` rejects reversed/forbidden production
crate edges. `cargo xtask perf` gates RSS, binary size, catastrophic timing
regressions and an actual release-profile source panic; detailed timings are
archived as JSON. Release packaging invokes the offline baseline first.

RELS-001 also accepts static PIE (`file`: static-pie linked; `ldd`: statically
linked). Unit cases reject dynamic/ambiguous tool output; the local acceptance
run additionally rejects a real dynamically linked ELF executable.
