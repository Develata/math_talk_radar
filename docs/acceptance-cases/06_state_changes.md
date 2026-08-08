# 06 — State & Change Detection

| ID | Requirement | Gate | Status |
|---|---|---|---|
| STATE-001 | first_seen persisted across scans | hard | pending |
| STATE-002 | second scan with no change emits no changes | hard | pending |
| STATE-003 | media_added emitted when a new video appears | hard | pending |
| STATE-004 | `--no-state` performs no write | hard | pending |

Plan ref: `docs/plan/07_state_change_detection.md`. Integration tests; STATE-003
is the canonical baseline (§23). Lands in M3.
