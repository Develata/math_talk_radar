# ADR-0005 — SourceSpec selectors for the configured HTML adapter

- Status: Accepted
- Date: 2026-08-09
- Decider: Deve
- Supersedes: none

## Context

The configured HTML adapter (SRC-004, `AdapterKind::HtmlConfig`) parses
site-specific event list and detail pages with `scraper` CSS selectors. Unlike
RSS/ICS/JSON-LD, there is no standard schema the parser can assume — each
institution calendar uses different markup. The adapter therefore needs the
selectors carried alongside the source definition (§17) so the same adapter
code serves every configured-HTML source.

`SourceSpec` is loaded from `config/sources.toml` and is the source registry's
runtime shape. Adding selectors there keeps all per-source configuration in one
place and lets the fetch coordinator hand `&SourceSpec` to the adapter without a
second lookup. The public JSON schema is governed by §64: `schema_version =
"1.0"`, and v0.x may add optional fields without a bump.

## Decision

Add a pure-data `HtmlSelectors` struct to `radar-core/src/config.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HtmlSelectors {
    pub list: String,
    pub list_link: String,
    pub detail_title: String,
    pub detail_date: String,
    #[serde(default)]
    pub detail_location: Option<String>,
    #[serde(default)]
    pub detail_description: Option<String>,
    #[serde(default)]
    pub detail_speaker: Option<String>,
}
```

Add an optional carrier field to `SourceSpec`, after `fixture`:

```rust
#[serde(default)]
pub selectors: Option<HtmlSelectors>,
```

Only `AdapterKind::HtmlConfig` consults `selectors`; other adapters ignore it.
The four `list`/`list_link`/`detail_title`/`detail_date` fields are required at
the struct level (non-optional `String`) so a configured-HTML source with a
missing required selector fails fast at TOML load rather than silently
discovering nothing. The `detail_location`/`detail_description`/`detail_speaker`
fields are `Option<String>` because not every site exposes them; `detail_speaker`
being `Some` is what unlocks TALK-001 structured speaker extraction.

## Alternatives considered

- **Key selectors off `media_strategy`**: rejected. `media_strategy` is a free
  string for media-plane policy (§20), already `Option<String>`. Overloading it
  to encode selectors would require a second parsing pass, conflate two
  unrelated concerns, and make TOML validation opaque. Selectors deserve their
  own typed table.
- **Defer to M6 (site configs)**: rejected. M6 ships the audited site configs,
  but the adapter *mechanism* — reading selectors from `SourceSpec` and applying
  them — is M2 scope (SRC-004). Deferring the struct would block the
  HtmlConfigAdapter (Todo 7) and the SRC-004 acceptance case, and would force M6
  to touch `radar-core` for a purely structural addition. Adding the type now,
  with selectors absent from every real source until M6, is a no-op for existing
  behavior.
- **A separate `selectors.toml` file keyed by source id**: rejected. Splits one
  source's configuration across two files, complicates `cargo xtask check`
  validation, and gains nothing — `SourceSpec` already exists per source.
- **Required `selectors` for `HtmlConfig` at deserialization**: rejected as
  unenforceable in `radar-core`. `SourceSpec` is deserialized generically for all
  adapter kinds; a `#[serde(deserialize_with)]` rule keyed on `adapter` would
  couple core to adapter semantics. The "required when adapter is HtmlConfig"
  invariant is enforced by the adapter itself (returns `AdapterError::Parse` when
  `selectors` is `None`), matching the M2 plan.

## §64 compatibility rationale

`selectors` is `Option<HtmlSelectors>` with `#[serde(default)]`, so a `SourceSpec`
TOML without a `[selectors]` section deserializes to `selectors: None`. This is
strictly additive: no existing field is renamed, retyped, or removed, so the
public JSON schema stays at `"1.0"` and no compatibility test needs to change.
A round-trip test (`source_spec_deserializes_without_selectors_field`) pins this
behavior. The optional `detail_*` fields inside `HtmlSelectors` carry their own
`#[serde(default)]` so a `[selectors]` table with only the four required fields
still parses.

## Consequences

- `radar-core` gains one public type (`HtmlSelectors`) and one `SourceSpec`
  field. No new dependencies; `serde` is already in use. `radar-core` remains
  pure — it carries the selectors but does not apply them (that is
  `radar-adapters`, via `scraper`).
- `cargo xtask check` (source-registry validation) does not yet know about
  `selectors`; M6 will add validation that `AdapterKind::HtmlConfig` sources
  carry a non-empty `selectors` with the four required fields. Until then, an
  misconfigured source is caught at adapter runtime, not registry load.
- The `SourceAdapter::enrich` contract (§13) is unchanged: adapters receive
  `&SourceSpec` and may read `source.selectors`. No trait signature change.
- If a future schema bump removes or renames a selector field, §64 requires a
  `schema_version` bump plus a compatibility test; this ADR introduces no such
  breaking change.
