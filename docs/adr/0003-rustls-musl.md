# ADR-0003 — rustls + musl static release

- Status: Accepted
- Date: 2026-08-08
- Decider: Deve
- Supersedes: none

## Context

The release asset must run on a clean Ubuntu 22.04 container with no Rust,
Cargo, or OpenSSL dev packages (§51). A dynamically-linked OpenSSL would break
that.

## Decision

- `reqwest` is configured with `default-features = false` + `rustls-tls`. Never
  depend on system OpenSSL.
- The release target is `x86_64-unknown-linux-musl`, producing a fully static
  binary.

## Alternatives considered

- Native OpenSSL via `reqwest` defaults: rejected — dynamic dependency, breaks
  the clean-container requirement.
- `openssl` vendored build: rejected — heavier, slower build, unnecessary with
  rustls.

## Consequences

TLS behavior is rustls's. The musl target must be added before release
engineering (`rustup target add x86_64-unknown-linux-musl`, M7). The `ring`
crate (rustls's crypto backend) ships a C build script that requires a musl C
compiler, so the release environment must also install `musl-tools` and export
`CC_x86_64_unknown_linux_musl=musl-gcc` before `cargo build`. Acceptance:
`file` + `ldd` show no runtime dynamic-library deps (RELS-001).
