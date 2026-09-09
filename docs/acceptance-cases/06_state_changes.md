# 06 — State & Change Detection

| ID | Requirement | Gate |
| --- | --- | --- |
| STATE-001 | first_seen persisted across scans | hard |
| STATE-002 | second scan with no change emits no changes | hard |
| STATE-003 | media_added emitted when a new video appears | hard |
| STATE-004 | `--no-state` performs no write | hard |

Plan ref: `docs/plan/07_state_change_detection.md`. Integration tests; STATE-003
is the canonical baseline (§23). Lands in M3.

STATE-001/002/003 regressions: all-source scan authority, including a healthy
source's absent event protected by an unrelated source failure; cancellation
after complete recovery; provenance/media retention over successive partial
scans; one-shot cancellation; owned-vector allocation reuse; alias first_seen
and tombstone transfer; source-health/change-log retention and reopen; v4
migration. CLI integration uses two independent fixture sources: failed B
protects both its historical events and disappeared events from healthy A.
