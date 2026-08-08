# 07 — Update & Uninstall

| ID | Requirement | Gate | Status |
|---|---|---|---|
| UPD-001 | `update --check` writes nothing | hard | pending |
| UPD-002 | checksum failure preserves the working binary | hard | pending |
| UPD-003 | valid update atomically replaces the binary | hard | pending |
| UPD-004 | broken candidate triggers rollback | hard | pending |
| UNS-001 | `--dry-run` mutates nothing | hard | pending |
| UNS-002 | `--keep-data` preserves only data | hard | pending |
| UNS-003 | `--purge` removes all app-owned paths | hard | pending |
| UNS-004 | unmanaged/development binary is protected | hard | pending |

Plan ref: `docs/plan/10_update_uninstall.md`. All verified in a temporary
sandbox; never against the real install. Lands in M5.
