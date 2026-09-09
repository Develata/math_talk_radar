# Tool-enabled verification follow-up — 2026-09-09

Evidence only. Supplements the earlier optimization acceptance snapshot; the
original measurements remain historical. No remote CI, installation of the
application, or publication was performed.

## Findings and corrections

1. `cargo deny check` detected RUSTSEC-2026-0258 in locked h2 0.4.15.
   Updated only h2 to 0.4.16 (commit `73e802d`), the advisory's patched version.
   Advisory: https://rustsec.org/advisories/RUSTSEC-2026-0258.html.
   The follow-up check passed advisories, licenses, bans and sources. Existing
   duplicate-version and workspace-path wildcard warnings remain warnings.
2. Coverage reproduced an unmanaged-binary protection bug: the directory
   heuristic recognized only target/debug and target/release. The coverage
   target directory bypassed it and uninstall deleted the shared test executable,
   causing additional lifecycle tests to fail. Only a generated build artifact
   was affected, not an installed application or user data.
   Update and uninstall now require a manifest matching the resolved binary
   unless --force-unmanaged is explicit, as required by plan section 36.
   No path spelling establishes ownership. The path heuristic was removed.
3. Lifecycle tests now copy the executable into a temporary custom build
   directory. The uninstall regression failed before the fix and passed after
   it. Absent/stale manifests preserve the copied binary and app directories;
   the new update regression also proves no release HTTP request is made.

## Coverage

`cargo llvm-cov --workspace --lcov --output-path target/coverage.lcov` passed:
426 tests. `cargo llvm-cov report --summary-only` reported:

| Metric | Covered / total | Percent |
|---|---:|---:|
| Lines | 9086 / 10653 | 85.29% |
| Functions | 939 / 1116 | 84.14% |
| Regions | 14815 / 17385 | 85.22% |

This is the default workspace report, not branch coverage or a production-only
coverage claim. Inline unit-test code contributes to the report. Several
command/xtask entrypoints are not exercised by the instrumented test suite;
separate ordinary xtask runs do not increase these coverage measurements.
No percentage gate was lowered or introduced to fit this result.

## Validation

The 12 lifecycle sandbox tests passed. `cargo machete` found no unused
dependencies. The final combined change passed `cargo xtask baseline` (426
tests, formatting, strict all-target/all-feature Clippy, architecture/document
checks, GNU release build, performance/resource gates and release panic
isolation), `cargo check --workspace --all-targets`, and `git diff --check`.
The final report addition also passed `cargo xtask check`.

## Static release follow-up

The user installed musl-tools; LLVM tools and the x86_64-unknown-linux-musl
Rust target are also installed. The previously missing static validation is
now complete locally:

- `cargo build --offline --locked --release --target x86_64-unknown-linux-musl`
  passed. The application source is unchanged from `87bd2ee`.
- The resulting 12,321,024-byte (11.75 MiB) binary is static PIE. `readelf`
  shows no PT_INTERP and no DT_NEEDED entries; `file` reports static-pie linked,
  while ldd exits successfully with statically linked.
- This reproduced a false rejection in both the old xtask and release shell
  gate. The corrected validator accepts traditional static ELF and static PIE,
  checks inspection success, removes filenames from file output, and rejects
  empty/ambiguous/dynamic dependency output. Real `/bin/true` was rejected.
  Release CI now invokes that same validator and explicitly installs musl-tools.
- Docker's daemon was unavailable. A SHA-256-verified official Ubuntu Base
  22.04.5 rootfs was run under Bubblewrap with isolated filesystem/process/user
  namespaces and no host library bind mounts. Its network namespace was shared
  solely to reach a host loopback RSS fixture server; no live website was used.
  The rootfs had no Rust, Cargo or libssl-dev installed.
- Help, version, schema golden equality, source listing, two actual fixture
  scans and temporary redb persistence passed. Each scan returned one healthy
  source and one event; EventId and first_seen_at were stable, and the second
  scan emitted no changes. Only temporary test state was written.
- After the gate correction, `cargo xtask baseline` passed all 428 tests,
  formatting, strict Clippy, architecture/document checks and performance
  gates. The two added tests cover static/static-PIE acceptance and rejection
  of dynamic, non-ELF, empty and ambiguous inspection output.

Binary SHA-256:
`c0ad97ace8b26063edf86dbdc17ab4045917c88050f1787735c70871631613b3`.

Rootfs: [official Ubuntu Base 22.04.5 amd64 archive](https://cdimage.ubuntu.com/ubuntu-base/releases/22.04/release/ubuntu-base-22.04.5-base-amd64.tar.gz),
verified against the publisher's HTTPS SHA256SUMS. SHA-256:
`242cd8898b33ea806ef5f13b1076ed7c76f9f989d18384452f7166692438ff1a`.

Earlier coverage percentages above remain the measured snapshot before the
xtask-only gate correction; coverage was not remeasured for those helper changes.
Remote workflow execution, release provenance attestation and publication
remain unperformed. The application was not installed or pushed remotely.
