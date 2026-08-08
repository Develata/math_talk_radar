# math_talk_radar

A radar for discovering public mathematics conferences, talks, lecture series,
recordings, slides, and related resources. It performs deterministic collection
and coarse ranking; interpretation and summarization are intentionally left to
downstream humans or AI agents.

v0.1 is a pure Rust CLI — no LLM, no browser automation, no JS runtime.

## Status

Pre-release. M0 (repository bootstrap) complete; see
`docs/report/implementation-status.md`.

## Install

A static musl binary is published with each release. Download from the
[releases page](https://github.com/Develata/math_talk_radar/releases), verify the
SHA-256, and place it on your `PATH`. Or build from source:

```bash
cargo build --release
```

## Quick start

```bash
math_talk_radar scan --after 180 | jq
math_talk_radar sources list
math_talk_radar doctor
math_talk_radar schema
```

`stdout` is structured JSON (schema `"1.0"`); `stderr` carries logs.

## Configuration

User config lives under `$XDG_CONFIG_HOME/math_talk_radar/` (default
`~/.config/math_talk_radar/`). See `config/` for examples:

- `sources.toml` — source definitions (M6 promotes audited entries).
- `scholars.toml` — scholar aliases (decoupled from any parser).
- `topics.toml` — canonical topics + aliases.
- `interests.example.toml` — interest weights that adjust ranking only.

See `docs/reference/config-schema.md`.

## Self-update & uninstall

```bash
math_talk_radar update --check
math_talk_radar update
math_talk_radar uninstall --dry-run
math_talk_radar uninstall --keep-data --yes
```

Self-update verifies SHA-256, keeps a rollback copy, and never deletes the
working binary on failure. Uninstall deletes only known app-owned paths and
protects `cargo run` development binaries unless `--force-unmanaged` is given.

## Development

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo xtask check          # source-registry + acceptance-matrix + doc coverage
cargo xtask check-matrix   # acceptance-matrix structural validation
```

We work on `main` with one atomic commit per verified milestone (M0–M8). No
feature branches, no PRs.

## Documentation

- Engineering contract: `docs/plan/00_engineering_constitution.md`
- Roadmap: `docs/tasks/implementation-roadmap.md`
- Acceptance matrix: `docs/registry/acceptance-matrix.tsv`
- Runbook: `docs/runbook.md`
- ADRs: `docs/adr/`

## License

MIT (`LICENSE`).
