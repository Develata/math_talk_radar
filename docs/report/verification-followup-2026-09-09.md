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

LLVM tools and x86_64-unknown-linux-musl Rust standard libraries are now
installed. The local system still lacks musl-gcc; sudo requires the user's
password, so musl-tools installation and static build/linkage/container
validation remain pending. No system privilege settings were changed.
