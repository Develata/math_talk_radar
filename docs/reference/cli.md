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
math_talk_radar state migrate --from OLD_DB --to NEW_DB
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

## redb 2 → 4 upgrade (proposed; ADR-0018 approval pending)

Existing databases use redb file format v2. The new engine requires format v3;
the application schema stays v4. `scan` refuses the old file and prints migration
instructions. It does not delete or reset history.

1. Stop all radar processes and keep the previous binary for rollback.
2. Run `math_talk_radar state migrate --from /path/state.redb --to /path/state-v3.redb`.
   The destination must not exist. Allow disk space for a complete additional
   database. Conversion and validation happen on a private copy; the original
   stays byte-for-byte unchanged, including on failure.
3. Use `scan --state /path/state-v3.redb` in future invocations and schedules.
   Confirm the retained history before switching your normal invocations.

To roll back, stop the new binary and run the previous binary with the original
database. Do not use a redb-4-written file with the previous binary: reverse
compatibility is not guaranteed. The retained original does not contain later
scans made against the new file; keep both if those observations matter. No
automatic downgrade or history merge is provided. Application v1-v3 state
migrations still run transactionally on the new copy. Future schemas and
malformed rows are rejected. `--no-state` remains a deliberate stateless scan,
not a migration or recovery method.
