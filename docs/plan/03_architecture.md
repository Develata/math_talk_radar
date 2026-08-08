# 03 — Architecture

> Status: M0 skeleton. Authoritative for crate boundaries, stack, layout. §10,
> §11, §22, §33, §37, §38, §39, §40.

## Workspace layout (§10)

```
crates/{radar-core,radar-fetch,radar-adapters,radar-state}
apps/cli
xtask
config/{sources,scholars,topics,interests.example}.toml
tests/{fixtures,golden,integration}
docs/{plan,acceptance-cases,registry,adr,reference,tasks,report}
.github/{workflows,dependabot.yml}
```

## Crate DAG (§11)

```
radar-core ← (pure domain, no I/O)
radar-fetch    → radar-core   (HTTP only)
radar-adapters → radar-core   (pure parsing, no network)
radar-state    → radar-core   (persistence only)
cli → core, fetch, adapters, state   (only composition root)
```

Forbid: core→{reqwest,redb,scraper}; adapters→reqwest; state→reqwest;
fetch→scraper. Enforce via Cargo dependencies.

## Technology stack (§37)

Rust 2024, stable. tokio, clap, reqwest (rustls, no OpenSSL), scraper, feed-rs,
chrono, chrono-tz, redb, serde/serde_json/toml/schemars, blake3, sha2, semver,
tracing, thiserror (libs), anyhow (CLI composition).

## Dependency policy (§38)

std first → small maintained crate → large framework last. `Cargo.lock`
committed. `cargo deny check` (advisories, license, banned, duplicate, registry).

## Unsafe policy (§39)

`#![forbid(unsafe_code)]` in every crate. No `unsafe` without explicit USER
approval.

## File cohesion (§40)

Hand-written production code: ~300 lines → soft review, >500 → hard warning
(justified only for tables/generated code/grammars). Do not split mechanically.

## State (§22)

Embedded local DB (redb, ADR-0002). Store canonical fingerprints, first/last
seen, source health, change-detection state, schema version. Never store full
HTML, videos, cookies, or tokens.

## XDG layout (§33)

Binary `~/.local/bin/math_talk_radar`; config `$XDG_CONFIG_HOME/math_talk_radar`;
data `$XDG_DATA_HOME/math_talk_radar`; cache `$XDG_CACHE_HOME/math_talk_radar`.
Never write a dotdir in `$HOME` root.

## Acceptance cases

- CFG-001 — embedded default config.
- CFG-002 — invalid config fails closed.
