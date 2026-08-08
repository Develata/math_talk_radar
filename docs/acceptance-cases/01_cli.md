# 01 — CLI / Config / Exit Codes

| ID | Requirement | Gate | Status |
|---|---|---|---|
| CLI-001 | `--help` complete | hard | pending |
| CLI-002 | `--version` performs no network/state init | hard | pending |
| CLI-003 | `scan` stdout is pure JSON | hard | pending |
| CLI-004 | stderr/stdout are separated | hard | pending |
| CFG-001 | embedded default config exists | hard | pending |
| CFG-002 | invalid config fails closed | hard | pending |
| HTTP-004 | partial source failure → exit 0 | hard | pending |
| HTTP-005 | zero usable sources → exit 4 | hard | pending |

Plan refs: `docs/plan/09_cli_output_contract.md` (CLI), `docs/plan/03_architecture.md`
(CFG), `docs/plan/05_fetching_reliability.md` (HTTP-004/005). Automation: `cargo
test --test integration`. Lands in M4.
