# 01 — CLI / Config / Exit Codes

| ID | Requirement | Gate | Status |
|---|---|---|---|
| CLI-001 | `--help` complete | hard | pass |
| CLI-002 | `--version` performs no network/state init | hard | pass |
| CLI-003 | `scan` stdout is pure JSON | hard | pass |
| CLI-004 | stderr/stdout are separated | hard | pass |
| CFG-001 | embedded default config exists | hard | pass |
| CFG-002 | invalid config fails closed | hard | pass |
| HTTP-004 | partial source failure → exit 0 | hard | pass |
| HTTP-005 | zero usable sources → exit 4 | hard | pass |

Plan refs: `docs/plan/09_cli_output_contract.md` (CLI), `docs/plan/03_architecture.md`
(CFG), `docs/plan/05_fetching_reliability.md` (HTTP-004/005). Automation: `cargo
test --test integration`. Lands in M4.
