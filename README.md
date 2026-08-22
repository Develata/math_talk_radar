# math_talk_radar

A radar for discovering public mathematics conferences, talks, lecture series,
recordings, slides, and related resources. It performs deterministic collection
and coarse ranking; interpretation and summarization are intentionally left to
downstream humans or AI agents.

v0.1 is a pure Rust CLI — no LLM, no browser automation, no JS runtime.

## Status

v0.1.0 release candidate. 65/65 acceptance cases pass; the release gates cover
formatting, linting, workspace tests, acceptance checks, dependency policy,
coverage, MSRV, synthetic pipeline baselines, static musl build, and clean
Ubuntu runtime smoke. See `docs/report/implementation-status.md`.

## Install

The prebuilt v0.1 asset targets **`x86_64-unknown-linux-musl` only**. Download
it from the [releases page](https://github.com/Develata/math_talk_radar/releases),
verify the accompanying SHA-256 (and GitHub build-provenance attestation), then
place it on your `PATH`. Other targets should build from source:

```bash
cargo build --release
```

## Quick start

```bash
math_talk_radar scan --after 180 | jq
math_talk_radar sources list
math_talk_radar doctor
math_talk_radar schema
math_talk_radar update --check
math_talk_radar uninstall --keep-data --dry-run
```

`stdout` is structured JSON (schema `"1.0"`); `stderr` carries logs.

## Configuration

The binary ships embedded source, scholar, and topic registries. `scan` can
override the source/scholar registries and optionally load interest weights via
explicit CLI paths. Persistent app data/config follows the XDG locations where
app-owned files are needed. See `config/` for the shipped registries/examples:

- `sources.toml` — source definitions (16 audited sources enabled: 5 RSS,
  11 HTML-config).
- `scholars.toml` — scholar aliases (decoupled from any parser).
- `topics.toml` — canonical topics + aliases.
- `interests.example.toml` — interest weights that adjust ranking only.

Source configuration is fail-closed: unknown fields, duplicate IDs, invalid
enabled entrypoints, zero budgets/depths, and unsupported media strategies are
rejected before scanning. v0.1 supports only `media_strategy =
"youtube_channel"` on RSS sources.

See `docs/reference/config-schema.md` and `docs/reference/cli.md`.

## Self-update & uninstall

```bash
math_talk_radar update --check
math_talk_radar update
math_talk_radar uninstall --keep-data --dry-run
math_talk_radar uninstall --keep-data --yes
```

Self-update verifies SHA-256, keeps a rollback copy, and never deletes the
working binary on failure. Unsupported prebuilt targets are rejected before
network/filesystem mutation. Uninstall deletes only known app-owned paths and
protects `cargo run` development binaries unless `--force-unmanaged` is given.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo xtask check          # source-registry + acceptance-matrix + doc coverage
cargo xtask check-matrix   # acceptance-matrix structural validation
cargo xtask baseline       # tests + quality + RSS memory + 1k/5k/10k pipeline
cargo deny check           # supply chain (licenses + advisories + bans + sources)
```

The full baseline records wall time, peak RSS, redb size, JSON/JSONL streaming,
10k high-collision dedup behavior, release binary size, and CLI startup. The
existing RSS parser memory gate remains ≤128 MiB; the scale baselines are
recorded as growth evidence rather than arbitrary micro-SLAs.

## Documentation

- Engineering contract: `docs/plan/00_engineering_constitution.md`
- Roadmap: `docs/tasks/implementation-roadmap.md`
- Acceptance matrix: `docs/registry/acceptance-matrix.tsv`
- Source registry: `docs/registry/source-registry.tsv`
- Runbook: `docs/runbook.md`
- ADRs: `docs/adr/`

## License

MIT (`LICENSE`).
