# CLI Reference

> Authoritative shape lives in `apps/cli/src/cli.rs` and
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
| `--max-events` | — | optional cap on emitted events |
| `--max-talks` | 300 | per-event talk cap; explicit `--max-talks 0` disables the cap |
| `--timezone` | local IANA | override timezone |
| `--today` | system clock | inject date (YYYY-MM-DD) for deterministic runs |
| `--sources` | embedded registry | override `sources.toml`; parsed and semantically validated before scan |
| `--scholars` | embedded registry | override `scholars.toml` |
| `--interests` | none / neutral | optional user interest-weight TOML |
| `--state` | app data path | override redb state path |
| `--no-state` | false | do not read or write state |
| `--format` | `json` | `json` \| `jsonl` |
| `--detail` | `compact` | `compact` \| `full` |

`--sources` does not enable arbitrary extra configuration surface: v0.1 accepts
the documented source schema only, rejects unknown keys, and fails closed on
semantic errors such as duplicate IDs, unsupported media strategies, or an
enabled source without an HTTP(S) entrypoint.

## Exit codes

0 success (incl. partial source failure) · 2 usage · 3 config/schema · 4 zero
usable sources · 5 state fatal · 6 output serialization fatal · 10 update ·
11 uninstall.
