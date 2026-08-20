# Runbook

> Operational recipes. M0 documents the intended procedures; commands become
> functional as their milestones land.

## Daily scan

```bash
math_talk_radar scan --after 180 | jq
```

`stdout` is structured JSON; `stderr` carries logs. Save with:

```bash
math_talk_radar scan --after 180 \
  --interests ~/.config/math_talk_radar/interests.toml \
  > radar.json
```

## Deterministic replay (testing)

```bash
math_talk_radar scan --today 2026-08-08 --no-state --format json
```

Same fixture + config + `--today` → stable ordering, IDs, scores (§49).

## Source health

```bash
math_talk_radar sources list
```

> `sources check` is a deferred stub (ADR-0009, post-v0.1). For live source
> health, run `cargo xtask live-smoke` (LIVE-003, advisory).

## Diagnostics

```bash
math_talk_radar doctor
math_talk_radar doctor --json
```

> `doctor` is local-only in v0.1 (no `--network` flag).

## Lifecycle

```bash
math_talk_radar update --check
math_talk_radar update
math_talk_radar uninstall --keep-data --dry-run          # plan only, no mutation
math_talk_radar uninstall --keep-data --yes              # noninteractive
math_talk_radar uninstall --purge --yes
# TTY without --yes: interactive three-way menu (§35.1)
```

## Source redesign workflow (§63)

1. live-smoke detects failure (or manual report);
2. reproduce locally;
3. refresh the sanitized fixture under `tests/fixtures/`;
4. update the adapter + targeted tests;
5. bump `last_verified` in `docs/registry/source-registry.tsv`;
6. `cargo xtask check` → `cargo xtask baseline`;
7. commit.

Never hack a selector against live HTML without adding a fixture.
