# 07 — Update & Uninstall

| ID | Requirement | Gate | Status |
|---|---|---|---|
| UPD-001 | `update --check` writes nothing | hard | pass |
| UPD-002 | checksum failure preserves the working binary | hard | pass |
| UPD-003 | valid update atomically replaces the binary | hard | pass |
| UPD-004 | broken candidate triggers rollback | hard | pass |
| UNS-001 | `--dry-run` mutates nothing | hard | pass |
| UNS-002 | `--keep-data` preserves only data | hard | pass |
| UNS-003 | `--purge` removes all app-owned paths | hard | pass |
| UNS-004 | unmanaged/development binary is protected | hard | pass |

Plan ref: `docs/plan/10_update_uninstall.md`. All verified in a temporary
sandbox; never against the real install. Lands in M5.
