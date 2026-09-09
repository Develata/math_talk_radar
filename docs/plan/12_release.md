# 12 — Release

> Status: M0 skeleton. Authoritative for release engineering. §34.1, §50, §51,
> §52, §53.

## Release profile (§50)

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "unwind"
strip = "symbols"
```

Modifiable by ADR if benchmarks justify it. ADR-0014 requires unwind so source
task panics remain isolated in production, with release-profile verification.

## Static release (§51)

Canonical asset: `x86_64-unknown-linux-musl`. Acceptance: `file` + `ldd` show no
runtime dynamic-library deps; runs on a clean Ubuntu 22.04 container with no
Rust/Cargo/OpenSSL-dev.
Both traditional static ELF and static PIE are valid. `cargo xtask static-release`
owns the linkage check for local validation and release CI; dynamic binaries,
failed inspection commands, and ambiguous dependency output must be rejected.

## Supply chain (§52)

ADR-0015 replaces incidental serial ordering with dependencies: independent
fmt/clippy, tests, coverage, cargo-deny, acceptance/architecture and performance
checks may run concurrently with the musl build. All remain required. Static
check, clean-Ubuntu smoke and checksum verification consume that exact musl
artifact. An always-running fan-in rejects missing, skipped, failed, cancelled,
stale or mismatched required evidence before attestation/publication. Downloaded
release assets are explicitly rehashed before publication. Minimal permissions;
pin third-party actions by full SHA; Dependabot for Cargo + Actions. Release
must not skip the complete baseline, even if another workflow was green.

Preserve the remote coverage gates: radar-core line coverage at least 85%,
and workspace line coverage at least 75%. Both commands produce run-local JSON
reports and are required by the coverage receipt.

## Acceptance orchestration

The existing Rust xtask owns plan/run/summarize. Case definitions under
`docs/acceptance-cases/` own the required IDs; registry rows map them to checks,
not historical success. Receipts are per-run artifacts with source, toolchain,
profile, module/case/shard, fixture digest, command, timing and exit status.
The CLI public schema remains `1.0`; development evidence has its own version.

Selection is whole-crate plus every reverse consumer, using all Cargo dependency
kinds and conservative target/feature unions. Shared configuration, public
contracts, authority, workspace/dependencies, toolchain, CI/acceptance rules and
unknown changes force full. Invalid or incomplete case/shard/graph catalogs fail
closed. Main CI, daily full, release and future deployment/restore always run
full. Local selective execution is allowed on the main-only checkout. PR CI
initially runs in shadow (plans selection, executes full); no branch workflow is
introduced. Manual full is the recovery path when a trusted change base is absent.

The first selector deliberately treats every non-Rust input (including fixtures
and golden data) as shared by the whole workspace. This conservative input edge
avoids inferring I/O dependencies from strings or stale compiler dep-info. Core
domain definitions, state authority, lifecycle trust, scan composition and CLI
public surfaces also force full. Only private Rust implementation/test changes
with a known crate owner may use the smaller reverse-dependency closure. More
precise fixture selection requires a separately verified input-consumer catalog.
MSRV is derived from Cargo metadata, not duplicated in workflow configuration.

Each application instance owns its temporary XDG directories, state database,
identity, writable fixtures and logs; mock servers bind OS-assigned ports.
Runner and program paths are runtime inputs. A receipt is valid only for its
source snapshot, run/attempt, command and actual artifact hashes. Missing receipts
and zero/ignored tests cannot be converted into passes. Human security review
remains a release requirement and must reference the release source commit.

Do not add Docker packaging or remote per-case shards at this stage. Reuse a
build only within an identical toolchain/target/profile/features/lock/flags tuple.
If packaging or restore is later introduced, use the already verified binary
digest, verify packaged read-back, and restore a specified snapshot into a new
directory. Cache entries never serve as acceptance evidence.

## release.yml (§53)

Trigger: `tag v*`. Must check `tag version == Cargo.toml version`; fail otherwise.

Manual dispatch is a nonpublishing preflight (ADR-0017). Its clean-checkout
`preflight` profile runs the full baseline plus build, artifact and attestation
checks, selecting the four automated release cases but not SEC-003. It uses the
same signer workflow and exact artifact provenance rules. Final preflight
success does not approve a release. Tag-triggered releases still require the
commit-bound human review. Publication is restricted to version-tag push events.

## Assets (§34.1)

`math_talk_radar-x86_64-unknown-linux-musl` + `.sha256` (required); `.tar.gz`,
SBOM (optional).

## Acceptance cases

- PERF-002 — binary ≤30 MiB (release).
- RELS-001 — static musl binary (container).
- RELS-002 — checksum asset (release).
- RELS-003 — artifact provenance (release).
