# 06 — State & Change Detection

| ID | Requirement | Gate |
| --- | --- | --- |
| STATE-001 | first_seen persisted across scans | hard |
| STATE-002 | second scan with no change emits no changes | hard |
| STATE-003 | media_added emitted when a new video appears | hard |
| STATE-004 | `--no-state` performs no write | hard |

Plan ref: `docs/plan/07_state_change_detection.md`. Integration tests; STATE-003
is the canonical baseline (§23). Lands in M3.

STATE-001/002/003 regressions: per-event A/B authority (including unknown
provenance), failure of unrelated C, A+B disappearance after both recover,
provenance/media retention over successive partial scans, one-shot cancellation,
owned-vector allocation reuse, alias first_seen/tombstone transfer and reopen
compatibility. CLI integration uses two independent fixture sources: failed B
retains its historical events while healthy A can cancel its disappeared events.
