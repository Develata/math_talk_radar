# Runbook

> Operational recipes for the implemented CLI and acceptance runner.

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
math_talk_radar scan --no-state --format json > radar-health.json
jq '.source_health' radar-health.json
```

ADR-0009 defers `sources check` and network diagnostics in `doctor`. Source
health comes from a scan. Live smoke records external availability as advisory;
invalid commands, malformed output, and execution failures still fail the job.
An all-sources-failed scan may exit 4 without JSON; its health ratio is unknown.

## Diagnostics

```bash
math_talk_radar doctor
math_talk_radar doctor --json
```

## Lifecycle

```bash
math_talk_radar update --check
math_talk_radar update
math_talk_radar uninstall --dry-run
math_talk_radar uninstall --keep-data --yes   # noninteractive
math_talk_radar uninstall --purge --yes
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

## Acceptance and CI

The Rust xtask owns selection and verdicts. The case documents define required
IDs; the TSV registry maps them to executable checks. Neither is a pass report.
Each run binds the source digest, Cargo lockfile, toolchain, build flags, runner
digest, run/attempt and expected check set. Commands, logs and test identities
are retained alongside receipts. Missing, skipped, stale or mismatched evidence
fails aggregation. Cargo caches are only a build optimization.
Acceptance performance probes write directly into their shard directory;
the legacy standalone `perf` command still defaults to `target/perf-latest.json`.

On Linux, install the repository toolchain and `llvm-tools-preview`, the MSRV
from workspace `rust-version`, cargo-nextest 0.9.143, cargo-deny 0.20.2 and
cargo-llvm-cov 0.9.1. Coreutils `timeout`, Python 3.12+ and Git are also needed.
Fetch dependencies before offline execution. Cargo-deny may refresh its advisory
database over the network; tests themselves use fixtures and mock servers.
The baseline, acceptance test/coverage/performance children and standalone smoke
set both `NO_PROXY` and `no_proxy` to `localhost,127.0.0.1,::1`. This keeps fixture
requests off inherited HTTP proxies; product and live-scan proxy behavior is
unchanged. For direct `cargo test` or `cargo run --example perf_scan`, supply those
variables yourself when your shell has proxy settings.

For a full local run, create a fresh directory and copy the runner before
planning. Do not edit source files or replace that runner during execution:

```bash
cargo fetch --locked
cargo build --locked -p xtask
run_dir="$(mktemp -d "$PWD/target/acceptance-local.XXXXXX")"
# Default Cargo target directory; adapt this copy if CARGO_TARGET_DIR is set.
cp target/debug/xtask "$run_dir/xtask"
"$run_dir/xtask" acceptance plan --profile full --out "$run_dir/plan.json"
"$run_dir/xtask" acceptance run --plan "$run_dir/plan.json" --shard all --out "$run_dir/receipts"
"$run_dir/xtask" acceptance summarize --plan "$run_dir/plan.json" --receipts "$run_dir/receipts" --out "$run_dir/summary.json"
```

`--workspace-root /absolute/checkout` can precede the xtask command when invoking
it elsewhere. Plans, shard directories and summaries cannot be reused. After an
edit or failed run, create a new run directory. A successful `full` summary covers
the development baseline, not release or live cases. `cargo xtask baseline`
remains available for the original local baseline without the receipt protocol.

For selective work, replace the plan command with `--profile local --base <sha>`.
Choose a known verified ancestor as the base; the runner does not establish that
an arbitrary base was green. Use `--base HEAD` to consider only working tree
changes. Missing or unusable bases force full execution. Known private Rust
changes select their crate and every reverse consumer; non-Rust inputs, fixtures,
public contracts and unknown paths force full. `--profile shadow --base <sha>`
records that selection but executes full, including failures outside selection.

Main pushes, scheduled CI and manual CI run full. Existing PR events run shadow;
development remains main-only. The DAG has quality, tests, assurance and
performance lanes. Release adds build, artifact and review dependencies. The
always-running gate rejects any required lane that did not succeed. Cache saves
are restricted to main and contain build outputs, not acceptance receipts.

Use **Re-run all jobs** in GitHub after a failure. A new attempt requires a new
plan and all evidence from that attempt; re-running only failed jobs deliberately
cannot reuse the previous attempt's artifacts. Inspect `acceptance-summary-*`
and `receipts-*-*` artifacts for machine summaries and command logs. Failed setup
may prevent a Rust summary from being produced; the workflow still fails closed.

## Release evidence

The release tag must match the workspace version. Release planning requires a
clean checkout and executes full acceptance. One musl build produces both the
product binary and a standalone smoke runner. The artifact lane checks linkage,
the 30 MiB bound and hashes, then exercises help, version, schema, diagnostics,
fixture scans, state reopening and `--no-state` in the pinned Ubuntu 22.04 image.
Docker is a release verification dependency, not product packaging or runtime.

A maintainer must record the actual SEC-003 human review in the repository
variable `RELEASE_REVIEW`, bound to the exact release source commit. It is a JSON
object with exactly these fields (replace placeholders after review):

```json
{
  "schema_version": 1,
  "commit": "<full release source commit SHA>",
  "case": "SEC-003",
  "status": "approved",
  "reviewer": "<maintainer identity>",
  "evidence": "<review record URL or precise reference>"
}
```

This declaration is not an automated review. Missing or wrong-commit review
blocks release. The pre-attestation summary excludes only attestation; the final
summary also requires verification of the exact subjects, source commit and
signing workflow. Publication consumes those verified bytes, then downloads both
published assets and compares hashes again. The published read-back receipt is
separate from the pre-publication acceptance summary; a read-back failure cannot
undo publication and leaves the workflow failed for maintainer investigation.

Local standalone smoke can use `xtask artifact-smoke <absolute-binary> <sha256>
<output.json>`, but a host run does not establish clean Ubuntu compatibility.
`--profile live` selects build plus advisory live checks; it never replaces full
acceptance or turns external availability into deterministic test evidence.
