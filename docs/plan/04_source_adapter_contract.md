# 04 — Source Adapter Contract

> Status: M0 skeleton. Authoritative for adapters. §13, §14, §17, §18, §19,
> §20, §45, §63. Implemented in `radar-adapters`.

## Adapter trait (§13)

```rust
trait SourceAdapter: Send + Sync {
    fn discover(&self, doc: &FetchedDocument, src: &SourceSpec)
        -> Result<Vec<EventStub>, AdapterError>;
    fn plan_enrichment(&self, event: &EventStub, src: &SourceSpec) -> Vec<FetchPlan>;
    fn enrich(&self, event: EventStub, docs: &[FetchedDocument], src: &SourceSpec)
        -> Result<EventCandidate, AdapterError>;
}
```

Parsers perform **no network I/O**. The coordinator + `radar-fetch` hand
prepared `FetchedDocument`s to these methods.

## Priority (§P-5)

official JSON/API → RSS/Atom/JSON Feed → ICS → JSON-LD Event → site-specific
HTML selectors → generic HTML fallback. The generic `<a>` parser is last resort
only (§74).

## Crawl boundary (§14)

`max_depth = 2` default: entry → event detail → program/media page. No
open-ended in-site crawl. Each source configures `allowed_hosts`, `max_depth`,
`request_budget`. URLs outside the allowlist are not fetched. Media offsite
links are recorded, not followed.

## Source registry (§17)

`docs/registry/source-registry.tsv` fields: id, name, tier, kind, adapter,
entrypoint, allowed_hosts, max_depth, request_budget, media_strategy, dynamic,
enabled, fixture, last_verified, status, notes. Validated by `cargo xtask check`.

## Coverage baseline (§18)

Audited ≥ 20, enabled ≥ 10, structured adapter kinds ≥ 2 (v0.1; restore to
≥ 3 when a qualified JSON-LD/ICS source is audited — see ADR-0007),
media/recording ≥ 3. Audit the real entrypoint before writing the URL.

## Dynamic sources (§19)

JS-only sources with no static data: `dynamic = true`,
`status = unsupported_dynamic`, `enabled = false`. Never add headless Chrome to
reach a source count. The audit itself has value even if the source is not
scrapable.

## Media discovery plane (§20)

Two planes: Event Plane (institution/calendar/conference → Event/Talk) and
Media Plane (official recordings/feed/archive → MediaResource → linked to
Event/Talk). Do not rely on conference list pages alone.

## Fixture policy (§45, §63)

Every enabled source: ≥1 list fixture, ≥1 detail fixture (if detail), ≥1 golden
expectation. On redesign: reproduce → refresh sanitized fixture → update adapter
→ targeted tests → bump `last_verified` → baseline → commit. Never hack a
selector against live HTML without a fixture.

## Acceptance cases

- SRC-001..005 — RSS, ICS, JSON-LD, configured HTML, generic fallback.
- SRC-006..008 — detail depth ≤2, host allowlist, request budget (mock server).
- MED-001..003 — video detection, slides detection, public access status.
