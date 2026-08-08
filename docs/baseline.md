# Baseline Framework

> Five baseline categories (§57). `cargo xtask baseline` orchestrates B1–B4;
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
