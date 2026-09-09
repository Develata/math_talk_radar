# 07 — Update & Uninstall

| ID | Requirement | Gate |
| --- | --- | --- |
| UPD-001 | `update --check` writes nothing | hard |
| UPD-002 | checksum failure preserves the working binary | hard |
| UPD-003 | valid update atomically replaces the binary | hard |
| UPD-004 | broken candidate triggers rollback | hard |
| UNS-001 | `--dry-run` mutates nothing | hard |
| UNS-002 | `--keep-data` preserves only data | hard |
| UNS-003 | `--purge` removes all app-owned paths | hard |
| UNS-004 | unmanaged/development binary is protected | hard |

Plan ref: `docs/plan/10_update_uninstall.md`. All verified in a temporary
sandbox; never against the real install. Lands in M5.

UNS-004 also exercises a copied executable in a custom build directory with
absent and stale manifests. The binary and application directories must survive.
Update rejects the same unmanaged paths before making any release HTTP request.
