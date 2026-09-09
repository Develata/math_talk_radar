# Baseline Framework

> Five baseline categories (§57). `cargo xtask baseline` runs tests, quality/meta checks and local performance;
> static linkage, supply chain and deployment checks have separate commands/CI.
> B5 is a live monitoring metric, not a CI hard gate.

## B1 Functional

CLI, date, adapter, people, topic, media, dedup, state. Driven by golden +
fixture + integration tests.

## B2 Quality

`cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features --
-D warnings`, `cargo test --workspace`, coverage, `cargo deny check`,
`forbid(unsafe_code)`, acceptance-matrix coverage.

## B3 Performance

Startup (`--version`/`--help` < 100ms), offline 20-source mock scan, peak RSS
(≤ 128 MiB offline), binary size (≤ 30 MiB).

## B4 Release

musl static build, clean Ubuntu 22.04 run, checksum, self-update sandbox,
uninstall sandbox, artifact attestation.

## B5 Live Source

Audited source count, enabled source count, success ratio, median source
latency, parse-error list, last verification date. Advisory only — third-party
outages must not fail normal CI.

## Reproducible local metrics

`cargo xtask perf` runs the RSS fixture probe, synthetic 10k processing/state
probe, actual 20-source mock scan and release panic isolation, binary-size and
startup checks. Reports live in Cargo's target directory as `perf-latest.json`,
with revision/time, method IDs and hard-gate failures. Timings are advisory;
resource/catastrophic gates and deterministic complexity tests fail CI. Normal
CI archives the JSON artifact; release packaging runs the complete offline
baseline. `docs/report/baseline-latest.md` is an August historical snapshot,
not generated output. See `docs/report/optimization-before-2026-09-09.md` for the
pre-change evidence and methodology limitations.
