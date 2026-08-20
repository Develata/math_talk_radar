# CLI Reference

> Status: M0 skeleton. Authoritative shape lives in `apps/cli/src/cli.rs` and
> `docs/plan/09_cli_output_contract.md`.

Binary: `math_talk_radar`.

## Commands

```
math_talk_radar scan        [options]   # discover + rank events (stdout: JSON)
math_talk_radar sources list|check [id] # inspect the source registry
math_talk_radar doctor      [--json]
math_talk_radar update      [--check] [--force-unmanaged]
math_talk_radar uninstall   [--dry-run] [--keep-data|--purge] [--yes] [--force-unmanaged]
math_talk_radar schema                  # print the public JSON output schema
```

> `sources check` is a deferred stub (ADR-0009, post-v0.1); it returns
> `not_implemented` in v0.1. `doctor` has only `--json` (no `--network`).

## Global options

- `-v` / `--verbose` — repeat for more detail (`-v` info, `-vv` debug).
- `--log-format text|json` — log output format (stderr).

## scan options

| Option | Default | Notes |
|---|---|---|
| `--mode` | `both` | `upcoming` \| `recordings` \| `both` |
| `--before` | 30 | days before `--today` |
| `--after` | 180 | days after `--today` |
| `--jobs` | 8 | concurrent fetch jobs |
| `--max-events` | — | cap emitted events |
| `--max-talks` | — | cap emitted talks |
| `--timezone` | local IANA | override timezone |
| `--today` | system clock | inject date (YYYY-MM-DD) for deterministic runs |
| `--sources/--scholars/--interests/--state` | XDG defaults | file path overrides |
| `--no-state` | false | do not read or write state |
| `--format` | `json` | `json` \| `jsonl` |
| `--detail` | `compact` | `compact` \| `full` |

## Exit codes

0 success (incl. partial source failure) · 2 usage · 3 config/schema · 4 zero
usable sources · 5 state fatal · 6 output serialization fatal · 10 update ·
11 uninstall.
