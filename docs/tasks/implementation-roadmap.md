# Implementation Roadmap

> Milestones M0–M8 (§72). One atomic commit per verified milestone on `main`.

## M0 — Repository Bootstrap

Workspace, AGENTS, plan skeletons, acceptance skeleton, matrix, CI skeleton,
LICENSE, README skeleton. Gate: `cargo check` + `cargo xtask check-matrix`.

## M1 — Core Domain

models, date, normalization, people, topics, identity, ranking primitives. Gate:
core tests + date/person golden + clippy.

## M2 — Fetch + Adapters

HTTP policy/concurrency/timeout/retry, RSS/ICS/JSON-LD/configured HTML/generic
fallback. Gate: offline mock + fixtures + fault injection.

## M3 — State + Dedup + Changes

redb schema/migrations, dedup, first_seen, media_added, change detection. Gate:
state integration + dedup golden + change tests.

## M4 — CLI

scan, sources, doctor, schema, output contract. Gate: assert_cmd integration +
stdout/stderr + exit codes + JSON schema.

## M5 — Lifecycle

self-update (checksum + rollback), uninstall (dry-run/keep-data/purge) in
sandbox. Gate: UPD/UNS sandbox cases.

## M6 — Live Source Audit

Audit ≥ 20, enable ≥ 10 with fixtures + golden + live smoke. Gate: LIVE-001/002.

## M7 — Release Engineering

musl, release workflow, checksum, attestation, clean Ubuntu smoke, coverage,
cargo-deny, Dependabot. Gate: RELS + PERF-002.

## M8 — Final Acceptance

Run the full baseline; review matrix, registry, baseline-latest, README. Tag
`v0.1.0` and release only when all hard gates pass.
