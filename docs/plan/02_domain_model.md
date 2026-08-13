# 02 — Domain Model

> Status: M0 skeleton. Authoritative for types. §5, §6, §7, §8, §9, §21, §24.
> Implemented in `radar-core`.

## Hierarchy (§P-3)

```
EventSeries → Event → Talk / Session → MediaResource
```

## Event (§5.1)

`Event { id, title, event_type, status, date, location, description, topics,
people, talks, media, access, sources, score, score_components, rank_reasons,
first_seen_at, last_seen_at }`.

## EventType (§5.2)

conference, workshop, research_program, public_lecture, distinguished_lecture,
lecture_series, summer_school, mini_course, colloquium, panel, award_lecture,
memorial_conference, seminar, unknown. `memorial_conference` is NOT junk if it
has a real program.

## Talk (§5.3)

`Talk { id, title, speaker: Vec<PersonHit>, date_time, abstract_text, topics,
media, source }`.

## MediaResource (§5.4, §20)

`media_type`: video, audio, slides, lecture_notes, transcript, program_pdf,
abstract_pdf, livestream, playlist, other. The program only records links +
metadata — **never downloads media files**.

## Person model (§6)

`PersonHit { canonical_name, matched_text, role, evidence, confidence,
scholar_tags }`. Roles (§P-2): speaker, lecturer, organizer, participant,
panelist, honoree, series_namesake, title_mention, unknown.

## Date model (§8)

`EventDate { start, end, timezone, original_text, precision }`. MVP must parse
range/US/cross-month/ISO forms. Filtering uses interval overlap:
`event.start <= query.end AND event.end >= query.start`. Unparseable dates are
retained with `precision = unknown` (lower rank), not dropped. Clock injection:
`--today`, `--timezone` (§8.4).

## Lifecycle (§9)

announced, registration_open, upcoming, ongoing, completed, media_pending,
media_available, archived, cancelled, unknown. `recording_expected` only when
the page shows explicit evidence.

## Access (§21)

`PublicAccess`: open, registration_required, institution_login, paywalled,
in_person_only, unknown. `OnlineAvailability`: livestream, hybrid,
recording_available, recording_expected, no_online_access, unknown. Open
recording is a key ranking signal.

## ID contract (§24)

IDs are deterministic: `BLAKE3(normalized canonical identity fields)`. Event:
`normalized_title + canonical_url` (v0.1; see ADR-0008). Never random UUID,
timestamp, vector index, or output order. Required for change detection.

> **v0.1 derivation (ADR-0008):** the plan originally specified
> `normalized_title + canonical organizer/domain + start_date`, but no v0.1
> adapter parses a structured `organizer` field and `start_date` is frequently
> absent at discover time, so the v0.1 code hashes `normalized_title +
> canonical_url` instead. A future schema bump may incorporate `organizer`
> and `start_date` once those fields are reliably parsed across adapters.

## Acceptance cases

- DATE-001..005 — date parsing + interval overlap + unparsed retention.
- TALK-001 — talk + speaker extraction.
