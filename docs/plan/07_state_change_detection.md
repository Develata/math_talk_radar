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

`event_updated` fires when a persisted event's title, date, location,
description, talk set, or people set changed. The talk-set and people-set
checks ensure that removed talks/speakers — which have no dedicated
`*Removed` change kind in this contract — are surfaced via `event_updated`
rather than vanishing silently from the change log.

Canonical baseline: first scan sees an event with `media=[]`; second scan sees
the same event with a new video → must emit `media_added`.

## State compatibility (§65)

Independent `state_schema_version`. Migrations are transactional; a failure
leaves no half-migrated state. Destructive migrations must be explicit.
`first_seen` and media history must not be silently lost (state is rebuildable
but history should not vanish without notice).

Ordered persisted keys must use a canonical fixed-width UTC timestamp
representation. Chrono's variable-precision default RFC3339 formatting is not
permitted for lexicographically ordered state keys because values within the
same second can sort incorrectly. The canonical form is RFC3339 UTC with nine
fractional digits (`SecondsFormat::Nanos`, `Z`).

`SOURCE_HEALTH` keys are `{source}\0{fixed_timestamp}`. `CHANGE_LOG` keys are
`{fixed_timestamp}\0{event_id}\0{kind}\0{detail_digest}`. The detail component
must be a deterministic fixed-size digest rather than unbounded external text,
while still distinguishing multiple same-kind changes for one event in one
scan. State schema v4 transactionally re-keys v1-v3 data into this canonical
form and fails closed on malformed rows or key collisions (ADR-0013).

## Acceptance cases

- STATE-001 — first_seen persisted (integration).
- STATE-002 — second scan unchanged (integration).
- STATE-003 — media_added (integration).
- STATE-004 — `--no-state` no write (integration).

## Scan ownership and absence authority (ADR-0016)

The CLI consumes fetched candidates, enriches and deduplicates them, then gives
state an owned current vector and the complete source-health slice. State stamps
first_seen/last_seen in place and returns that same vector. One previous corpus
supports change detection; borrowed complete-snapshot APIs remain available.

Cancel absent previous events only when every supplied source status is Ok.
Any non-Ok status, including Partial, suppresses the entire prune step and all
EventCancelled records. An empty health slice asserts a complete snapshot.
Disabled sources and unknown provenance are pruned in a complete scan; an
unrelated source outage protects all absent events. Preserve failed-source
provenance and missing media/talks across incomplete observations when previous
supporting sources lack complete coverage.

State schema v4 and its transactional migration remain authoritative. Current
dedup input-ID aliases transfer previous first_seen and unexpired tombstones to
the corrected representative inside the same write transaction, coalescing
history by the earliest timestamp. No guessed/fuzzy historical aliases are
introduced. Ranking runs after state reconciliation; new scan snapshots retain
compatible ranking fields at adapter defaults. Removing those derived fields
requires a separately justified projection/migration. Source-health and change
history are committed atomically with events and retained for 90 days.
