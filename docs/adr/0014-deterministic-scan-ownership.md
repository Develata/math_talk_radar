# ADR-0014 — Deterministic identity, crawl accounting, and scan authority

- Status: Accepted for implementation; absence/cancellation policy in decision 5
  superseded by ADR-0016 after explicit user confirmation on 2026-09-09
- Date: 2026-09-09
- Authorization: current user engineering-optimization request, priorities P0–P2
- Scope: correction of ranking-dependent identity and incomplete-scan deletion;
  no public JSON or on-disk schema change

## Evidence and decision

1. `merge_events` selected scalar fields by user-interest score, and `run_scan`
   scored before and after deduplication. An interest-only change could select
   another ID/title/URL and change subsequent clustering. Use ascending stable
   EventId, URL, and source provenance as the canonical ordering; lexical scalar
   ties remain independent of ranking. Equal ordering keys retain stable input
   order. Do not invent a source-quality policy based on presentation weights.
   Enrich, dedup, then rank once. Keep the title+canonical-URL hash unchanged.
2. Greedy deduplication scans all previous representatives. Index the existing
   four identity signals and choose the earliest matching cluster, regardless
   of signal priority across different clusters. Remove superseded keys after
   merging; do not turn this into transitive graph clustering. Keep a test-only
   linear reference and deterministic differential/complexity tests.
3. A shared robots OnceCell charges its initializing source's content budget.
   Robots transactions instead have a separate bound of redirect_limit + 1
   physical requests per cache initialization, under the same body limits,
   host permits and deadline. Cache keys retain origin + allowed-host policy.
   SourceHealth.requests counts content attempts, including retries/redirects.
4. Source task count currently scales with source count, and missing-result
   detection is quadratic. Admit at most jobs tasks, replenish on completion,
   and retain deterministic source ordering and individual failure reports.
   Release builds must unwind so task panics can actually be isolated.
5. `store_scan` unconditionally cancels absent events, including during partial
   outages. The CLI must supply authoritative source IDs (only status Ok).
   Absence is authoritative for an event only when its nonempty supporting
   provenance is entirely authoritative. Unknown/disabled/failed sources retain
   their previous events. Preserve provenance across partial observations so
   a temporary outage does not erase a source's later veto.
6. Move current events from fetch results through persistence; stamp the owned
   vector in place. Keep the stored Event format and state schema version.
   Preserve first_seen history when the corrected representative is an existing
   candidate alias; do not discard old state merely because ranking once chose
   a different candidate. Redundant persisted ranking fields remain until a
   separately justified schema migration.

7. The fetch-side candidate cap runs only after adapters materialize their
   entire stub vector. Move the shared limit to the core adapter contract and
   stop stub construction at limit + 1; the extra stub preserves fetch's Partial
   overflow signal and the existing first-2000 selection. DOM/feed parsing stays
   bounded by the HTTP body cap, and existing parser depth guards remain.

## Boundaries, compatibility, and verification

The existing core ← fetch/adapters/state ← CLI DAG remains unchanged. Core owns
identity and dedup; fetch owns physical requests, robots and scheduling; state
owns transactional mutation; CLI owns health-to-authority composition, ranking
and output policy. No new framework or runtime dependency is required.

Regression evidence covers interest invariance, same-host tiny budgets,
first-cluster semantics with changing keys, bounded task admission/panic
isolation, per-event source authority and history preservation. Existing golden,
schema, lifecycle and offline fixture tests remain mandatory. Performance probes
use advisory timings plus wide resource and deterministic complexity gates.
