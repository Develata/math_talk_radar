# 07 — State & Change Detection

> Status: M0 skeleton. Authoritative for persistence. §22, §23, §65. Implemented
> in `radar-state`.

## State store (§22, ADR-0002)

Embedded `redb` database. Store: canonical event fingerprints, first_seen,
last_seen, talk fingerprints, media fingerprints, source-health history,
change-detection state, schema version. Never store full HTML, videos, cookies,
or auth tokens.

## Change kinds (§23)

`event_added`, `event_updated`, `schedule_added`, `speaker_added`,
`livestream_added`, `media_added`, `media_removed`, `event_cancelled`.

Canonical baseline: first scan sees an event with `media=[]`; second scan sees
the same event with a new video → must emit `media_added`.

## State compatibility (§65)

Independent `state_schema_version`. Migrations are transactional; a failure
leaves no half-migrated state. Destructive migrations must be explicit.
`first_seen` and media history must not be silently lost (state is rebuildable
but history should not vanish without notice).

## Acceptance cases

- STATE-001 — first_seen persisted (integration).
- STATE-002 — second scan unchanged (integration).
- STATE-003 — media_added (integration).
- STATE-004 — `--no-state` no write (integration).

## Scan ownership and absence authority (ADR-0011)

The CLI consumes fetched candidates, enriches and deduplicates them, then gives
state an owned current vector and the set of complete source IDs (status Ok).
State stamps first_seen/last_seen in place and returns that same vector. It keeps
one previous corpus for change detection; the legacy borrowed complete-snapshot
API remains available, but the CLI uses the owned authority-aware path.

Cancel an absent previous event only if it has nonempty provenance and all its
supporting sources were authoritative. Missing, disabled, failed or partial
sources do not establish absence. An unrelated source outage does not veto
cancellation. Preserve failed-source provenance across partial observations;
otherwise a later scan could lose the supporting source's veto. Conservatively
retain missing media/talks when previous event coverage is incomplete, because
merged resources expose only one source, not all supporting sources.

The state schema remains v2 and existing Event JSON remains readable. Current
dedup input-ID aliases transfer previous first_seen and unexpired tombstones to
the corrected representative inside the same write transaction, coalescing
history by the earliest timestamp. No guessed/fuzzy historical aliases are
introduced. History whose old candidate is currently unavailable stays subject
to the conservative provenance rule. Ranking runs after state reconciliation;
new scan snapshots retain the compatible ranking fields at adapter defaults.
Removing those derived fields requires a separately justified projection/migration.
