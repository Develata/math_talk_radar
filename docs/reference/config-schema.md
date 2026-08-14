# Config Schema Reference

> Authoritative shapes: `config/*.toml` and `radar-core` config types
> (`SourceSpec`, `HtmlSelectors`, `ScholarSpec`, `TopicSpec`).

## sources.toml

`[[sources]]` entries mirror `SourceSpec` (§17). Each enabled source must
declare `allowed_hosts`, `max_depth`, `request_budget` (§14).

```toml
[[sources]]
id = "clay"
name = "Clay Mathematics Institute"
tier = "S"
kind = "conference_series"
adapter = "rss"
entrypoint = "https://www.claymath.org/events/rss"
allowed_hosts = ["www.claymath.org"]
max_depth = 1
request_budget = 30
media_strategy = ""
dynamic = false
enabled = true
```

### Fields

| Field | Type | Required | Default | Notes |
|---|---|---|---|---|
| `id` | string | yes | — | unique identifier |
| `name` | string | yes | — | human-readable name |
| `tier` | `S`\|`A`\|`B`\|`unknown` | no | `unknown` | quality tier |
| `kind` | enum | no | `other` | `institution_calendar`, `conference_series`, `rss_feed`, `ics_feed`, `indico`, `json_ld`, `media_archive`, `other` |
| `adapter` | enum | no | `none` | `rss`, `ics`, `json_ld`, `indico`, `html_config`, `html_generic`, `none` |
| `entrypoint` | URL | no | — | feed/list page URL |
| `allowed_hosts` | string[] | no | `[]` | host allowlist for fetch redirects |
| `max_depth` | u8 | no | `2` | max redirect/follow depth |
| `request_budget` | u32 | no | `60` | per-source HTTP request cap |
| `media_strategy` | string | no | `""` | media detection strategy hint |
| `dynamic` | bool | no | `false` | JS-rendered page flag |
| `enabled` | bool | no | `false` | whether the source is active |
| `selectors` | `HtmlSelectors` | no | — | required when `adapter = "html_config"` |

### HtmlSelectors (ADR-0005)

CSS selectors for `adapter = "html_config"`. `list`, `list_link`,
`detail_title`, `detail_date` are required; `detail_*` optional fields default
to absent; `list_title`/`list_date` (§P-5) are optional overrides.

```toml
[sources.selectors]
list = "div.event"
list_link = "a.event-link"
list_title = "h3.event-title"
list_date = "span.event-date"
detail_title = "h1"
detail_date = "time"
detail_location = ".location"
detail_description = ".abstract"
detail_speaker = ".speaker"
```

| Field | Type | Required | Notes |
|---|---|---|---|
| `list` | string | yes | container selector on list page |
| `list_link` | string | yes | link to detail page within each container |
| `list_title` | string | no | override title source (falls back to `list_link` text) |
| `list_date` | string | no | override date source (fed to `parse_date`) |
| `detail_title` | string | yes | title selector on detail page |
| `detail_date` | string | yes | date selector on detail page |
| `detail_location` | string | no | location selector |
| `detail_description` | string | no | description/abstract selector |
| `detail_speaker` | string | no | speaker name selector |

## scholars.toml (§6.1)

```toml
[[scholars]]
id = "don-zagier"
canonical_name = "Don Zagier"
aliases = ["Zagier", "Don B. Zagier"]
tags = ["wolf", "curated"]
```

Decoupled from any parser. The matcher enforces §6.2 ambiguity rules
(substring ≠ speaker; structured context required for `Speaker` role).

## topics.toml (§7)

```toml
[[topics]]
id = "arithmetic_geometry"
name = "Arithmetic Geometry"
aliases = ["arithmetic geometry", "Shimura varieties"]
```

Canonical topic + aliases; word-boundary matching (§7, `contains_phrase`).

## interests.example.toml (§7)

```toml
[interests]
arithmetic_geometry = 1.0
```

Weights are clamped to `[0.0, 1.0]` (CORE-17) and alter ranking ONLY — they
never delete events. NaN/Inf treated as neutral `1.0`; negative treated as
`0.0`.
