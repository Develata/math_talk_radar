# ADR-0008 — v0.1 `event_id` derivation uses title+URL, not title+org+date

- Status: Accepted
- Date: 2026-08-14
- Decider: Deve
- Supersedes: none (reconciles §24 plan text with v0.1 code; §24 text to be
  updated in a follow-up commit)

## Context

§24 (`docs/plan/02_domain_model.md`, "ID contract") specifies the event
identity hash as:

> `BLAKE3(normalized_title + canonical organizer/domain + start_date)`

The v0.1 implementation in `radar-core/src/model.rs::event_id` computes:

> `BLAKE3(normalized_title + canonical_url)`

This is CORE-15 from the round-4 pre-release audit: the plan names three
identity fields (title, organizer/domain, start_date) but the code names two
(title, canonical URL). The canonical URL embeds organizer/domain (host) and
frequently embeds a path/identifier that correlates with start_date, but it is
not the field set the plan promises.

### Why the code diverged

1. **Organizer is not parsed.** No v0.1 adapter extracts a structured
   `organizer` field; the `Event` model has no `organizer` slot. Adding one
   for identity purposes alone would inflate the schema and burden every
   adapter. The URL host is the closest available proxy and is already
   canonicalized by `normalize::canonicalize_url`.
2. **Start_date is frequently absent at discover time.** Many sources list
   events with date text the date parser cannot parse (`EventDate::unknown`).
   Including a missing date in the identity hash would collapse all
   undated events from the same host+title to one id. The URL (which often
   contains a per-event path slug) distinguishes them.
3. **Cross-adapter consistency.** §24 requires the same event discovered via
   different adapter kinds (RSS vs JSON-LD vs HTML) to produce the same id.
   Title+URL achieves this because the URL is the adapter-independent join
   key. Title+org+date would require every adapter to agree on the organizer
   canonical form, which is harder to guarantee than URL canonicalization.

### Risk of the current approach

Two events with the same title on the same URL (e.g. a listing page that
reuses one URL for a recurring series) collapse to one id. ADAP-12 mitigates
the JSON-LD case by synthesizing a per-event query param. The HTML-generic
and RSS adapters do not currently surface this because their stub URLs come
from per-item `<link>` / `href` attributes.

## Decision

1. Accept `BLAKE3(normalized_title + canonical_url)` as the v0.1 `event_id`
   derivation. It is the correct v0.1 projection given the available parsed
   fields.
2. Update §24 plan text to describe the actual v0.1 derivation, with a note
   that a future schema bump may incorporate `organizer` and `start_date`
   once those fields are reliably parsed across adapters.
3. Do NOT change the code before v0.1.0. Renaming the identity fields is a
   schema-level change (every persisted `EventId` would change), requiring a
   state migration and a public JSON schema bump — out of scope for the
   v0.1.0 tag.

## Consequences

- The v0.1 `event_id` is stable across scans and across adapter kinds as long
  as the canonical URL is stable.
- A future version that adds `organizer` / `start_date` to the identity hash
  will produce different ids and must ship a state migration + JSON schema
  bump. The `STATE_SCHEMA_VERSION` and public `schema_version` are the
  levers for that transition.
- ADAP-12 (unnamed JSON-LD events) and ADAP-13 (html_generic title vs
  event_id consistency) are downstream fixes that assume the current
  title+URL derivation; they remain valid under this decision.
