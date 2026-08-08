# ADR-0004 — Self-update trust & replacement model

- Status: Accepted
- Date: 2026-08-08
- Decider: Deve
- Supersedes: none

## Context

Self-update (§34) replaces the running binary from a GitHub Release asset. A
broken update must never leave the user without a working binary.

## Decision

Release origin is fixed to `Develata/math_talk_radar` (a single constant; change
only before the first release if the real remote differs). The update algorithm:

```
fetch latest stable → SemVer compare → download to sibling temp →
verify SHA-256 → chmod 0755 → fsync → run downloaded self-test →
create rollback copy → atomic replace → run replaced self-test →
delete rollback → fsync parent
```

Any failure leaves the current working binary usable. No downgrade. `--check` is
read-only. An independent lock prevents concurrent self-updates. The checksum
verifies download integrity; build provenance comes from the release workflow's
artifact attestation (§52).

## Alternatives considered

- In-place overwrite without checksum: rejected — corrupts the binary on a
  truncated download.
- Package-manager-only updates: rejected — v0.1 ships a standalone binary.

## Consequences

Uninstall (§35) must prefer the install manifest + `current_exe` to avoid
deleting unmanaged files, and must protect `cargo run` development binaries
unless `--force-unmanaged` is given (§36).
