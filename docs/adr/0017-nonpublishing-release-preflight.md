# ADR-0017 — Nonpublishing release preflight

- Status: Accepted for implementation
- Date: 2026-09-09
- Authorization: user requested arrangements and evidence for the remaining
  release/live acceptance cases after approving the proposed preflight route.

## Decision

Add a `preflight` acceptance profile requiring a clean source checkout, the full
baseline, one musl build, exact-artifact verification in clean Ubuntu, and
provenance attestation. It selects all automated release cases. SEC-003 remains
unselected: automated execution cannot manufacture the maintainer's review.

Manual dispatch of `release.yml` uses preflight. Tag pushes continue to use
`release`, including the commit-bound human review and tag/version check.
The publication job runs only for a version-tag push. Manual dispatch cannot
publish, even after successful attestation. Both paths verify attestations from
the existing `release.yml` signer and the exact source commit and subjects.

The Rust runner remains the sole owner of check/case selection and receipt
validation. Existing plan, manifest and receipt identity constraints apply;
profiles cannot exchange evidence. Preflight success describes automated
release readiness only; it is not a release approval or proof of publication.
The public product schema, state format and self-update trust model do not change.

## Evidence and failure handling

Archive the final preflight summary and all check receipts. A missing/failed
baseline, build, container check or attestation fails preflight. The live
workflow remains separate and advisory for source availability. A failed
command or malformed live output remains an execution failure.

Prepare SEC-003 as a reviewable source/logging audit with supporting tests.
Only a maintainer's actual approval may populate RELEASE_REVIEW, bound to the
final release source commit. Missing approval continues to block tag releases.

Regression tests must show that preflight selects the automated release cases
but not SEC-003, requires attestation in its final summary, rejects dirty input,
and cannot enter the publication job. Release still requires the review lane.
Removing the manual trigger and profile rolls back this feature without
changing runtime or persisted data. Normal CI remains full on main.
