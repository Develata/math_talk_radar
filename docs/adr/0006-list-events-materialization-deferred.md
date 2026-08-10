# ADR-0006 — Defer `list_events` materialization fix

- Status: Accepted
- Date: 2026-08-10
- Decider: Deve
- Supersedes: none

## Context

`Repository::list_events` (radar-state) materializes every stored event into a
`Vec<Event>` via a full table scan:

```rust
pub fn list_events(&self) -> Result<Vec<Event>, StateError> {
    let txn = self.db.begin_read()?;
    let table = txn.open_table(EVENTS)?;
    let mut out = Vec::new();
    for entry in table.iter()? {
        let (_, value) = entry?;
        out.push(serde_json::from_slice(value.value())?);
    }
    Ok(out)
}
```

A code review flagged this as a potential memory concern: with a large event
store, `list_events` could allocate a substantial `Vec` and hold all events in
memory simultaneously. The review proposed implementing a streaming/iterator
interface or pagination.

## Decision

Defer the fix. Keep `list_events` as a materializing `Vec<Event>` API for v0.1.
Revisit when event volume approaches the threshold below.

## Rationale

1. **Current scale is well within safe bounds.** v0.1 ships 15 enabled sources.
   Empirically each source yields 10–50 events, giving an expected store of
   ~150–750 events. Each `Event` serializes to roughly 2–8 KiB of JSON, so the
   full materialization costs ~1–6 MiB — well under the §57 RSS budget of
   128 MiB.

2. **The dominant caller is change detection.** `detect_changes(previous, current)`
   needs both snapshots in memory to diff by `EventId`. A streaming interface
   would require either an external merge-sort or buffering one side anyway,
   erasing most of the benefit for the primary use case.

3. **API churn cost is disproportionate for v0.1.** A streaming API would
   introduce an iterator trait, lifetime parameters on the read transaction,
   and downstream changes to every caller (scan engine, change detection,
   tests). v0.1 is in review-finalization; the risk/reward is unfavorable.

4. **No observed memory pressure.** The PERF-001 baseline (§57) passes with
   headroom; `list_events` has not surfaced as a hotspot in profiling.

## Revisit trigger

Revisit this decision when **any** of the following holds:

- Enabled sources exceed 100, or
- The event store exceeds 25,000 events, or
- `list_events` appears in a PERF baseline as a top-3 allocator, or
- A streaming use case (e.g. incremental change detection over a large
  historical store) becomes a product requirement.

At that point, preferred alternatives (in order):

1. Add a `list_events_iter()` returning `impl Iterator<Item = Result<Event>>`
   backed by the redb read cursor, keeping `list_events()` as a convenience
   wrapper that collects.
2. Add key-range pagination (`list_events_range(start..end, limit)`).
3. Move change detection to a streaming diff (external merge by `EventId`).

## Consequences

- `list_events` remains O(n) in memory. Callers must not invoke it in a hot
  loop expecting constant memory.
- The §57 RSS budget is not at risk at current scale.
- This ADR is the record of the deferral; the review item is resolved by
  documenting the trade-off rather than changing code.
