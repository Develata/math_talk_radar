# Source Registry Reference

> Status: M0 skeleton. Authoritative file: `docs/registry/source-registry.tsv`.
> Validated by `cargo xtask check`.

## Columns (§17)

`id`, `name`, `tier`, `kind`, `adapter`, `entrypoint`, `allowed_hosts`,
`max_depth`, `request_budget`, `media_strategy`, `dynamic`, `enabled`, `fixture`,
`last_verified`, `status`, `notes`.

## Enumerations

- **tier**: `S`, `A`, `B`, `unknown`.
- **kind**: `institution_calendar`, `conference_series`, `rss_feed`, `ics_feed`,
  `indico`, `jsonld`, `media_archive`, `other`.
- **adapter**: `rss`, `ics`, `jsonld`, `indico`, `html_config`, `html_generic`,
  `none`.
- **status**: `pending_audit`, `audited`, `enabled`, `disabled`, `broken`,
  `dynamic_unsupported`.
- **dynamic / enabled**: `true` / `false`.
- **max_depth / request_budget**: non-negative integers.

## M0 state

All 24 audited-candidate sources are `pending_audit`, `enabled=false`,
`adapter=none`, `tier=unknown`. Entrypoints are intentionally empty — the real
entrypoints are audited and filled in M6 (§18: audit the current entrypoint
before writing the URL).
