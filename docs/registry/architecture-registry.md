# Architecture Registry

Machine-readable companion to the crate DAG (§11). This file is human-readable;
the normative boundaries are enforced by the `Cargo.toml` dependency edges and
the `#![forbid(unsafe_code)]` lint in every crate.

## Crates

| Crate | Role | Depends on | Forbidden deps |
|---|---|---|---|
| `radar-core` | pure domain model + algorithms | (none) | reqwest, redb, scraper, tokio |
| `radar-fetch` | HTTP client + policy | radar-core | scraper, redb |
| `radar-adapters` | document parsers | radar-core | reqwest, redb, tokio |
| `radar-state` | persistence + change detection | radar-core | reqwest, scraper, tokio |
| `math_talk_radar` (cli) | composition root | core, fetch, adapters, state | (none — may use all) |
| `xtask` | dev tooling | (none) | (dev-only, not shipped) |

## DAG

```
radar-core
  ↑       ↑       ↑
fetch  adapters  state
   ↑       ↑       ↑
        cli
```

`cli` is the only composition root. No library crate may depend on `cli`.

## Validation

- `cargo check --workspace` — compilation respects the DAG.
- `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings.
- `#![forbid(unsafe_code)]` in every crate (SEC-001).
- `cargo deny check` (M7) — advisories, license, banned, duplicate, registry.
