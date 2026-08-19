# ADR-0012 — All-healthy guard before prune (P0-03)

- Status: Accepted (Deve sign-off 2026-08-19 — minimal fix: skip prune +
  suppress `EventCancelled` when any enabled source has terminal failure)
- Date: 2026-08-19
- Decider: Deve
- Supersedes: none
- Related findings: GPT-Pro audit P0-03 (round 10 pre-release audit)

## Context

`Repository::store_scan_bundle` (`crates/radar-state/src/repository.rs`) treats
events absent from the current scan as cancelled: it writes a
`CancelledEventTombstone`, removes the event row, and `detect_changes` emits
`EventCancelled` change records (§23, ST M-1 / INV-4).

That inference is only sound when the current scan is **complete** — i.e. every
enabled source actually contributed its events. When one or more enabled
sources have a terminal failure status (`Timeout`, `HttpError`, `ParseError`,
`RobotsDenied`, `DynamicUnsupported`, `BudgetExhausted`), the absent events are
simply missing from this scan, not genuinely cancelled. Pruning them would:

1. **Lose live events.** A source that recovers on the next scan would have to
   re-add the events, losing `first_seen_at` (the tombstone preserves it only
   within the 90-day retention window, and even then the event is briefly
   absent from query results between scans).
2. **Spam the change log.** Every transient source outage would emit a burst of
   `EventCancelled` records, followed by `EventAdded` records when the source
   recovers — noise that obscures genuine cancellation signals.

The scan path already distinguishes terminal failure from success via
`SourceStatus` on each `SourceHealth` (`scan_engine.rs:127-132` uses an
`any_usable` check to exit 4 when *all* sources fail). The prune step, however,
runs unconditionally inside `store_scan_bundle` — it has no notion of partial
source failure.

This is a GPT-Pro audit P0 finding (P0-03): "prune on partial failure loses
live events."

## Decision

Gate the prune step on an **all-healthy** precondition computed from the
`source_health` slice passed to `store_scan_bundle`:

- `all_healthy = source_health.iter().all(|h| matches!(h.status,
  SourceStatus::Ok | SourceStatus::Partial))`.
- When `all_healthy` is **false**:
  - Skip the prune loop (no tombstones written, no event rows removed).
  - Suppress `EventCancelled` records from the returned change vector (so the
    change log and stdout do not report spurious cancellations).
- When `all_healthy` is **true**: behavior is unchanged (prune + emit
  `EventCancelled` as before).
- An **empty** `source_health` slice (the legacy `store_scan` path, which
  delegates to `store_scan_bundle` with `&[]`) is vacuously healthy, so the
  legacy behavior is preserved.

`Ok` and `Partial` are the only non-terminal statuses — `Partial` means the
source fetched and parsed successfully but produced some warnings (e.g. a few
broken detail pages); its events are still present and authoritative.

## Rationale

1. **Minimal.** No schema change, no new API, no persisted-data semantics
   change that would require another §12 sign-off. The guard is a local
   branch inside the existing `store_scan_bundle` transaction.
2. **Correct invariant.** "Absent from this scan ⇒ cancelled" only holds when
   the scan is complete. The guard makes that precondition explicit instead
   of relying on the caller to withhold prune.
3. **Preserves legacy behavior.** `store_scan` (empty health slice) and the
   common all-healthy case are untouched.
4. **Disabled sources prune correctly.** A disabled source contributes no
   `SourceHealth` entry (the scan path only produces health for enabled
   sources). Disabling a source therefore does NOT trip the guard — its
   events are pruned as expected, which is the right behavior (the user
   explicitly disabled it).
5. **`last_seen_at` lag is acceptable.** When the guard skips prune, the
   absent events stay in the table but their `last_seen_at` is not advanced
   (only events present in the current scan get `last_seen_at = now`). This
   is correct: `last_seen_at` means "last confirmed seen," and a failed
   source did not confirm its events. When the source recovers, the events
   re-appear in the scan and `last_seen_at` advances. No staleness signal is
   lost.

## Consequences

- `store_scan_bundle` doc updated to document the conditional prune (step 4).
- `EventCancelled` change records are conditionally suppressed — callers that
  relied on the change vector always containing `EventCancelled` for absent
  events must account for the new condition. In practice, only the scan path
  consumes the change vector (it prints them to stdout) and `doctor`/`changes`
  read `CHANGE_LOG` — neither assumes `EventCancelled` is always present.
- No public JSON schema change (`schema_version` stays `"1.0"`).
- No `STATE_SCHEMA_VERSION` bump.
- A regression test is added: `store_scan_bundle_skips_prune_on_partial_failure`
  (seeds an event, scans with empty events + a `HttpError` health entry,
  asserts the event survives in the DB and no `EventCancelled` is returned).

## Revisit triggers

- A v0.2+ design that distinguishes "source down" from "source returned empty"
  at the `SourceStatus` level (e.g. a `Degraded` status for partial network
  failure) would refine the guard.
- A per-source prune policy (e.g. prune events from healthy sources even when
  others fail) would be a stronger fix but requires tracking which events came
  from which source in the persisted state — a larger change deferred to
  post-v0.1.
