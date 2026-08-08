# 02 — Dates

| ID | Requirement | Gate | Status |
|---|---|---|---|
| DATE-001 | same-month range (3–7 August 2026) | hard | pending |
| DATE-002 | cross-month range (31 August – 4 September 2026) | hard | pending |
| DATE-003 | US date format (August 3–7, 2026) | hard | pending |
| DATE-004 | interval-overlap filtering | hard | pending |
| DATE-005 | unparsed date retained with precision=unknown | hard | pending |

Plan ref: `docs/plan/02_domain_model.md`. Automation: `cargo test -p radar-core`.
Labeled baseline accuracy ≥ 98% (§47). Lands in M1.
