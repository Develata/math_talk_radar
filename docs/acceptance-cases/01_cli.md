# 01 — CLI / Config / Exit Codes

| ID | Requirement | Gate |
| --- | --- | --- |
| CLI-001 | `--help` complete | hard |
| CLI-002 | `--version` performs no network/state init | hard |
| CLI-003 | `scan` stdout is pure JSON | hard |
| CLI-004 | stderr/stdout are separated | hard |
| CFG-001 | embedded default config exists | hard |
| CFG-002 | invalid config fails closed | hard |
| HTTP-004 | partial source failure → exit 0 | hard |
| HTTP-005 | zero usable sources → exit 4 | hard |

Plan refs: `docs/plan/09_cli_output_contract.md` (CLI), `docs/plan/03_architecture.md`
(CFG), `docs/plan/05_fetching_reliability.md` (HTTP-004/005). Automation: `cargo
test --test integration`. Lands in M4.
