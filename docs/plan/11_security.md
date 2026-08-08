# 11 — Security

> Status: M0 skeleton. Authoritative for the threat model. §16, §39, §66, §67.

## Threat surface (§66)

The system handles untrusted HTML/XML/JSON, untrusted redirects, third-party
URLs, user-provided TOML, and GitHub Release metadata. Therefore:

- parsers must not panic;
- response bodies are capped;
- recursion/depth is capped;
- URL following is bounded by an allowlist;
- config has schema validation;
- remote input never decides filenames;
- update asset filenames are fixed;
- uninstall paths are fixed.

## HTTP security (§16)

HTTPS preferred; registry-bounded hosts; post-redirect policy re-check; no
cookies; no auth; no full-body persistence; no sensitive header logging;
`respect_robots = true`; no bypass.

## Unsafe policy (§39)

`#![forbid(unsafe_code)]` project-wide. No `unsafe` without explicit USER
approval.

## XML / feed safety (§67)

RSS/ICS/XML parsers must not load external entities, must not expand infinitely,
must cap body size, and a malformed feed affects only that source.

## No secret logging (§42, SEC-003)

Never log full HTML, full responses, user config contents, or tokens.

## Acceptance cases

- SEC-001 — no `unsafe` (lint, `forbid(unsafe_code)`).
- SEC-002 — `cargo deny check` (CI).
- SEC-003 — no secret logging (review/test).
