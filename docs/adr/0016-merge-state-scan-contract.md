# ADR-0016 — Integrate owned scans with v4 state and the all-healthy guard

- Status: Accepted (user confirmation 2026-09-09)
- Decision: preserve the remote pruning rule while merging local CI and
  performance changes.
- Supersedes: ADR-0014 decision 5 only for absence/cancellation authority.
- Preserves: ADR-0011 state history, ADR-0012 all-healthy guard, ADR-0013 v4 keys.

Every scan persists its observed events, source-health history and change log
in one transaction. An absent event may be cancelled only when every supplied
source-health status is `Ok`. Any non-`Ok` status, including `Partial`, blocks
all absence pruning and all `EventCancelled` records for that scan. An empty
health slice remains a complete-snapshot assertion, as in the remote API.
Disabled sources and unknown provenance do not create an additional veto when
the scan is otherwise complete.

The CLI passes the owned event vector and the full health slice. State stamps
the vector in place and retains the current dedup-alias transfer of first_seen
and unexpired tombstones. Consolidating an observed alias into its canonical
row is identity reconciliation, not absence pruning. Incomplete observations
still preserve failed-source provenance and missing media/talks when their
previous supporting sources lack complete coverage.

Keep state schema v4, its transactional v1-v3 migration, fixed timestamp keys,
bounded change-detail digests, and 90-day history retention. Public JSON stays
at 1.0. No database rows are migrated or removed by the repository merge itself.

Verification must cover a healthy source's missing event protected by an
unrelated source failure, cancellation after complete recovery, alias history
and allocation reuse, source-health/change-log read-back and retention, and
the existing v4 migration tests. CI retains the remote coverage thresholds
(85% core, 75% workspace) and full storage-performance workloads.
