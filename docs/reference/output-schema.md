# Output Schema Reference

> Status: M0 skeleton. Authoritative types: `apps/cli/src/output.rs` and
> `radar-core` model types. `schema_version = "1.0"` (§64).

## Envelope

```json
{
  "schema_version": "1.0",
  "generated_at": "2026-08-08T00:00:00Z",
  "query": { "mode": "both", "before_days": 30, "after_days": 180 },
  "events": [],
  "changes": [],
  "source_health": []
}
```

The top level is never a bare array (§29).

## Detail levels (§31)

- `compact`: event description ≤ 1200 chars, talk abstract ≤ 1200 chars.
- `full`: fields ≤ 8000 chars. Raw HTML is never emitted.

## Compatibility (§64)

v0.x may add optional fields. Renaming or removing a field requires a schema
bump plus a compatibility test. `math_talk_radar schema` prints the current
schema; CI checks Rust model ↔ generated schema ↔ golden output for drift.
