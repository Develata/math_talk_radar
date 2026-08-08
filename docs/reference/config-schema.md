# Config Schema Reference

> Status: M0 skeleton. Authoritative shapes: `config/*.toml` and
> `radar-core` config types.

## sources.toml

`[[sources]]` entries mirror `SourceSpec` (§17). Each enabled source must declare
`allowed_hosts`, `max_depth`, `request_budget` (§14). M0 ships an empty list;
M6 promotes audited entries.

## scholars.toml (§6.1)

```toml
[[scholars]]
id = "don-zagier"
canonical_name = "Don Zagier"
aliases = ["Zagier", "Don B. Zagier"]
tags = ["wolf", "curated"]
```

Decoupled from any parser. The matcher enforces §6.2 ambiguity rules.

## topics.toml (§7)

```toml
[[topics]]
id = "arithmetic_geometry"
name = "Arithmetic Geometry"
aliases = ["arithmetic geometry", "Shimura varieties"]
```

MVP uses canonical topic + aliases; no semantic model.

## interests.example.toml (§7)

```toml
[interests]
arithmetic_geometry = 1.0
```

Weights are in `[0.0, 1.0]` and alter ranking ONLY — they never delete events.
