# ADR-0018 — Explicit redb file-format migration

- Status: Proposed; requires user approval before publication
- Date: 2026-10-26
- Extends: ADR-0002, ADR-0013

## Context

PR #7 upgrades redb 2.6.3 to 4.4.0. The former creates file format v2 by
default; redb 3 and 4 only open format v3. The application's state schema v4
is independent of the engine's file format. Importing `ReadableDatabase`
fixes compilation but does not make existing databases readable.

## Proposed decision

Ordinary opens fail closed on format v2, with an actionable migration error.
`state migrate --from OLD --to NEW` requires explicit, distinct paths. It
locks OLD against concurrent writers, copies it into a private temporary file
beside NEW, upgrades only that copy through redb 2.6.4, runs the existing
transactional application migrations, validates all known records, and
publishes NEW without overwriting any existing destination. OLD remains
byte-for-byte unchanged. No data is recreated by scanning and no file-format
conversion is implicit in `scan`, `doctor`, or self-update.

Keep redb 2.6.4 solely as the migration bridge (its maintenance release fixes
the 2.6.3 cache-growth defect). Normal reads and writes use redb 4.4.0. Neither
the Rust 1.96 floor nor the security, coverage, or performance gates change.

## Compatibility and rollback

The five tables use `&str` keys and JSON byte values, except the `u32` schema
version. The upstream tuple/Option encoding changes do not apply. File-format
conversion preserves every table, including unknown tables. Application schema
v1-v3 still undergo the existing v4 migration on the copy; malformed rows and
future schema versions abort publication. No application schema or public JSON
schema bump is needed for the engine upgrade.

Users must opt in to the new database with `scan --state NEW`. Keep OLD and the
previous binary together for rollback. Do not promise that redb 2 can read a
database subsequently written by redb 4: upstream only supports this in some
cases. Rolling back to OLD does not include scans made only in NEW; retain NEW
as well if those later observations matter. There is no downgrade/merge tool.

This is a user-facing upgrade step and needs approval before shipping. A
binary-only rollback after migration is insufficient. Failure before publishing
NEW leaves OLD and any existing destination untouched. If the final directory
sync fails, the error explicitly says NEW was published; OLD is still intact.

## Evidence required

Regression tests cover v2 rejection without writes, migration of all five
tables and first_seen/media/history, old-binary reopen of OLD, candidate reopen
and subsequent writes, v1-v3 schema migration, malformed/future state refusal,
existing destinations, concurrent writers, and CLI exit codes. Run the existing
full quality, security, MSRV, coverage, and performance checks unchanged.

Upstream: [redb changelog](https://github.com/cberner/redb/blob/v4.4.0/CHANGELOG.md),
[2.6.3 upgrade implementation](https://github.com/cberner/redb/blob/v2.6.3/src/db.rs).
