# ADR-0007 — v0.1 adapter-kind baseline lowered to 2

- Status: Accepted
- Date: 2026-08-11
- Decider: Deve
- Supersedes: none (revises §18 baseline for v0.1 only)

## Context

§18 (`docs/plan/01_product_scope.md`, `docs/plan/04_source_adapter_contract.md`)
sets the v0.1 coverage baseline at "structured adapter kinds ≥ 3". At M7 close
the registry had exactly three enabled adapter kinds: `rss` (clay, ihes),
`html_config` (10 sources), and `json_ld` (stanford-math — the sole JSON-LD
source).

Audit of `stanford-math` found that its entrypoint `events.stanford.edu/`
serves all-campus events on the Localist/Concept3D platform. All 23 events in
the fixture are non-mathematical ("Take a break, pet a dog", "Financial
Counseling", "Surgery M&M"). The platform exposes no math-specific filter;
`math.stanford.edu/events/*` pages are 404 or JS-rendered shells. Keeping
stanford-math `enabled` would ship known non-math garbage into scan results,
violating §1 product positioning ("mathematics conferences, talks, lecture
series, recordings") and the §6 prohibition on silent destructive behavior.

Disabling stanford-math drops the enabled adapter-kind count from 3 to 2
(`rss` + `html_config`), which fails the `cargo xtask check` enforcement of
§18. The `json_ld` adapter implementation and its tests remain complete and
correct; the issue is solely the absence of a *qualified* JSON-LD source.

Alternative qualified JSON-LD or ICS candidates were audited and rejected:
Berkeley (`events.berkeley.edu/math`) is the same Localist platform returning a
JS shell; slmath, birs, oberwolfach, scgp, ias-math are
`dynamic_unsupported` or 403; harvard-math and berkeley-math have no fixture;
mathmeetings is broken; ems-calendar is `none` adapter.

## Decision

1. Disable stanford-math (`enabled = false`, `status = disabled`).
2. Lower the v0.1 §18 adapter-kind baseline from ≥ 3 to ≥ 2.
3. Record this as a v0.1-scoped revision, not a permanent relaxation. The
   `json_ld` adapter is retained; when a qualified JSON-LD or ICS source passes
   audit, the baseline should be restored to ≥ 3.

## Alternatives considered

- **Keep stanford-math enabled to satisfy ≥ 3.** Rejected. Ships known non-math
  garbage into v0.1 scan output. §23 ranking demotes `title_mention` matches but
  does not drop them, so the garbage would surface as low-scored events.
  Formal compliance with a coverage threshold cannot justify shipping bad data
  that violates §1 positioning.
- **Delay release to audit new candidates.** Rejected for v0.1 timing. All
  readily identifiable candidates were already audited and found unqualified.
  Further audit has unbounded time cost with uncertain yield; the remaining
  candidates (other university math dept pages, Indico instances) require full
  fixture → adapter → test → baseline cycles, not a configuration toggle.

## Consequences

- `cargo xtask check` threshold for `enabled_adapter_kinds` changes from `< 3`
  to `< 2`.
- §18 text in `01_product_scope.md` and `04_source_adapter_contract.md` is
  updated to "structured adapter kinds ≥ 2 (v0.1; restore to ≥ 3 when a
  qualified JSON-LD/ICS source is audited)".
- The `json_ld` adapter (`crates/radar-adapters/src/jsonld.rs`) and its
  `site_audits` test (`site_stanford_jsonld_discovers_events`) remain in tree,
  validating that the adapter still parses JSON-LD Event blocks correctly
  against the sanitized fixture. The test does not require the source to be
  enabled.
- LIVE-001 (≥ 20 audited) and LIVE-002 (≥ 10 enabled fixture-backed) are
  unaffected: stanford-math remains audited, and the disabled sources still
  count toward the 20 audited total.
- Future restoration: when a JSON-LD or ICS source is audited and enabled,
  revert this ADR's threshold change and restore §18 to ≥ 3 in the same commit
  that enables the new source.
