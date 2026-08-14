# 09 — CLI & Output Contract

> Status: M0 skeleton. Authoritative for the CLI surface. §27, §28, §29, §30,
> §31, §32, §33, §48, §64. Implemented in `apps/cli`.

## Commands (§27)

`scan`, `sources` (`list` | `check [id]` — deferred, see ADR-0009), `doctor`,
`update`, `uninstall`, `schema`.

## scan options (§27.2)

`--mode upcoming|recordings|both`, `--before <d>`, `--after <d>`, `--jobs <n>`,
`--max-events`, `--max-talks`, `--timezone <IANA>`, `--today <YYYY-MM-DD>`,
`--config/--sources/--scholars/--interests/--state <path>`, `--no-state`,
`--format json|jsonl`, `--detail compact|full`, `--verbose`, `--log-format
text|json`.

## stdout / stderr (§28)

`scan` stdout = machine-readable JSON only; logs/progress to stderr. Management
commands (`doctor`/`update`/`uninstall`/`sources`) may be human-readable by
default and offer `--json`.

## JSON envelope (§29)

Top level must NOT be a bare array. Must include `schema_version`.

```json
{ "schema_version": "1.0", "generated_at": "...", "query": {...},
  "events": [], "changes": [], "source_health": [] }
```

## Schema command + compatibility (§30, §64)

`math_talk_radar schema` prints the current schema. `schema_version = "1.0"`; v0.x
may add optional fields; renaming/removing requires a bump + compatibility test.
CI checks Rust model ↔ generated schema ↔ golden output have no drift.

## Detail levels (§31)

compact: description ≤1200 chars, abstract ≤1200 chars. full: fields ≤8000 chars.
Never emit raw HTML.

## Exit codes (§32)

0 success (incl. partial source failure) · 2 usage · 3 config/schema · 4 zero
usable sources · 5 state fatal · 6 output serialization fatal · 10 update ·
11 uninstall.

## Startup (§48)

`--version` / `--help` perform no network or state initialization. Target <100ms.

## Acceptance cases

- CLI-001 — `--help` complete (integration).
- CLI-002 — `--version` zero network (integration).
- CLI-003 — scan stdout pure JSON (integration).
- CLI-004 — stderr/stdout separated (integration).
