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

## Acceptance cases

- STATE-001 — first_seen persisted (integration).
- STATE-002 — second scan unchanged (integration).
- STATE-003 — media_added (integration).
- STATE-004 — `--no-state` no write (integration).
