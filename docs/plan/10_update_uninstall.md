# 10 — Update & Uninstall

> Status: M0 skeleton. Authoritative for lifecycle. §34, §35, §36, §66.
> Implemented in `apps/cli` (commands `update`, `uninstall`).

## Self-update (§34)

`math_talk_radar update --check` (read-only) and `math_talk_radar update`.
Default release origin: `Develata/math_talk_radar`.

### Asset contract (§34.1)

Release includes `math_talk_radar-x86_64-unknown-linux-musl` and its `.sha256`.
Optional `.tar.gz`, SBOM.

### Algorithm (§34.2)

Fetch latest stable → SemVer compare → download to sibling temp → verify SHA-256
→ chmod 0755 → fsync → run downloaded binary self-test → create rollback copy →
atomic replace → run replaced binary self-test → delete rollback → fsync parent.
Any failure leaves the current working binary usable. A failed download must
never delete the existing binary.

### Safety (§34.3)

HTTPS only; fixed release repo; no auto `sudo`; error if current executable not
writable; no downgrade; `--check` modifies nothing; scan never auto-updates;
independent update lock; stale temp files identifiable and cleanable. Checksum =
download integrity; build provenance comes from the release workflow attestation.

## Uninstall (§35)

`uninstall [--dry-run] [--keep-data | --purge] [--yes] [--force-unmanaged]`.

### Interactive (§35.1)

TTY: offer "program+config+cache, keep data" (default) / "remove everything" /
"cancel".

### Noninteractive (§35.2)

Requires explicit `--keep-data --yes` or `--purge --yes`, else refuse.

### keep-data (§35.3) / purge (§35.4)

keep-data removes binary + config + cache + install/update metadata + temp,
preserves `$XDG_DATA_HOME/math_talk_radar/`, and prints the preserved path.
purge removes everything app-owned. After purge, all app-created-and-registered
paths must not exist.

### Delete safety (§35.5)

Only known paths; canonicalize; never follow arbitrary symlink targets; never
recursively delete `$HOME` or `/`; fail closed on empty path; idempotent; no
shell `rm -rf`; use Rust fs APIs; no auto elevation. `--dry-run` prints the full
plan and changes nothing.

## Managed install metadata (§36)

Record official install path (binary, install_method, installed_version).
Uninstall prefers install manifest + `current_exe`. `cargo run` must not
silently delete the `target/debug` binary; unmanaged requires `--force-unmanaged`.

## Acceptance cases

- UPD-001 — update check no write (mock release).
- UPD-002 — checksum failure preserves binary (sandbox).
- UPD-003 — valid update replaces binary (sandbox).
- UPD-004 — broken candidate rollback (sandbox).
- UNS-001 — dry-run zero mutation (sandbox).
- UNS-002 — keep-data preserves only data (sandbox).
- UNS-003 — purge removes owned paths (sandbox).
- UNS-004 — unmanaged binary protection (sandbox).
