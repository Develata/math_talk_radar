# ADR-0013 — State v4 canonical ordered keys

- Status: Accepted (Deve sign-off 2026-08-23 — authorized v0.1.0 rewrite)
- Date: 2026-08-23
- Decider: Deve
- Supersedes: none; extends ADR-0011
- Related findings: v0.1.0 rewrite review — CHANGE_LOG same-kind collisions and RFC3339 lexical-order defect

## Context

ADR-0011 introduced append-only `SOURCE_HEALTH` and `CHANGE_LOG` tables whose
composite keys begin with timestamps so redb range scans can provide
chronological reads and retention purges without deserializing every value.
Two persistence defects remain before the rewritten first release:

1. Chrono's default `DateTime<Utc>::to_rfc3339()` emits variable fractional
   precision. Lexicographic order therefore does not necessarily equal time
   order for timestamps within the same second, invalidating range-query and
   retention assumptions.
2. A change-log key that omits a distinguishing detail component can overwrite
   multiple same-kind changes for the same event and scan. Storing raw detail
   text directly prevents that collision but creates unbounded externally
   influenced database keys.

Because v0.1.0 is explicitly being rewritten, this is the lowest-cost point to
correct the persisted schema rather than carrying compatibility debt forward.

## Decision

Bump `STATE_SCHEMA_VERSION` from 3 to 4 and enforce these key formats:

- Canonical timestamp: UTC RFC3339 with exactly nine fractional digits and `Z`
  (`to_rfc3339_opts(SecondsFormat::Nanos, true)`).
- `SOURCE_HEALTH`: `{source}\0{canonical_timestamp}`.
- `CHANGE_LOG`:
  `{canonical_timestamp}\0{event_id}\0{kind}\0{detail_digest}`.
- `detail_digest` is deterministic BLAKE3 over the optional detail string. It
  is fixed-size, preserves distinct same-kind records, and keeps repeated
  identical records idempotent.

The v3→v4 migration must rebuild both ordered tables from their serialized
values inside the same redb write transaction. Any iterator, storage,
deserialization, serialization, missing timestamp, or generated-key collision
error aborts the transaction and leaves the prior schema version unchanged.
The same fail-closed rule applies while importing bare legacy source-health
rows from v1/v2.

## Rationale

- Fixed-width UTC timestamps make lexicographic ordering equivalent to
  chronological ordering, including within one second.
- A fixed-size digest avoids both overwrite collisions and unbounded key
  growth from URLs, titles, speaker names, or other external detail strings.
- Rebuilding from serialized values avoids trusting malformed legacy keys.
- One transactional state-v4 migration preserves history and allows the
  rewritten v0.1.0 to freeze a correct persistence contract.

## Consequences

- Existing v1-v3 databases migrate automatically to v4 on writable open.
- Read-only open requires schema v4 after migration has occurred; newer schema
  versions continue to fail closed.
- Public output schema remains `1.0`; this is an internal persistence-schema
  change only.
- Regression coverage includes same-second ordering, fractional range queries,
  fractional retention cutoffs, v3→v4 lossless re-keying, malformed-row
  rollback, legacy-row rollback, and preservation of multiple same-kind change
  records across reopen.

## Revisit triggers

- Moving ordered keys to a binary integer timestamp representation.
- Introducing a state compaction/export format that no longer relies on redb
  lexical key ordering.
- Any future change to `ChangeRecord` identity semantics that requires more
  than timestamp/event/kind/detail to distinguish records.
