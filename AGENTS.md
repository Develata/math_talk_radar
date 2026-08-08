# AGENTS.md — `math_talk_radar` (root)

> Engineering contract: `docs/plan/00_engineering_constitution.md` and the
> SRS under `docs/plan/`. This file tells coding agents how to work in this
> repository. Project-level rules override the global agent policy on conflict.

## 1. Project purpose

`math_talk_radar` is a **radar for discovering public mathematics conferences,
talks, lecture series, recordings, slides, and related resources**. It performs
deterministic collection and coarse ranking; interpretation and summarization
are intentionally left to downstream humans or AI agents (§71, §76). v0.1 is a
pure Rust CLI — no LLM, no browser automation, no JS runtime.

## 2. Authority hierarchy (§0.2)

```
USER current explicit instruction
  → docs/plan/00_engineering_constitution.md
  → docs/plan/*.md
  → docs/reference/*.md
  → docs/acceptance-cases/*.md
  → docs/registry/*
  → code
  → docs/report/*
```

`docs/plan/` is the engineering design truth source. Code is a projection of the
plan. Acceptance cases are the verifiable proof. Reports record evidence only and
have no normative authority. If implementation reveals a plan assumption is
wrong, write an ADR first, then update the plan — never silently weaken the
contract because the code "already does it that way."

## 3. Crate dependency boundary (§11)

```
radar-core  ← pure domain, no I/O
radar-fetch    → radar-core   (HTTP only)
radar-adapters → radar-core   (pure document parsing, no network)
radar-state    → radar-core   (persistence only)
cli → radar-core, radar-fetch, radar-adapters, radar-state
```

- `radar-core` must NOT depend on `reqwest`, `redb`, or `scraper`.
- `radar-adapters` must NOT depend on `reqwest` (no network in parsers).
- `radar-state` must NOT depend on `reqwest` (no network in persistence).
- `radar-fetch` must NOT depend on `scraper` (no parsing business logic).
- `cli` is the only composition root.

## 4. Development workflow (§0.1)

- Work directly on `main`. No feature branches, no PRs.
- Commit per verified engineering milestone (M0–M8), one atomic commit per
  increment. Conventional Commits style.
- Never force-push, never rewrite submitted history, never `git reset --hard`
  over unrelated changes. Stage exact files only (no `git add -A`).

## 5. Work order (§0.3)

For any behavior-affecting task:

```
Read this AGENTS.md
  → read engineering constitution
  → read relevant plan
  → read relevant acceptance cases
  → implement
  → targeted tests
  → review
  → baseline
  → commit
```

## 6. Hard prohibitions

- No `unsafe` (every crate sets `#![forbid(unsafe_code)]`).
- No network I/O in `radar-core` or `radar-adapters`.
- No `as any` / `@ts-ignore`-style type suppression (Rust: no `unwrap`/`expect`/
  `panic!` in production code without a documented compile-time invariant).
- No real-website dependency in `cargo test` (§44). Tests use fixtures + mock
  servers.
- No generic `<a>` dump as a primary parser (§74). Generic HTML is last resort.
- Name substring ≠ speaker (§P-2, §6.2). Title mentions stay `title_mention`.
- No silent destructive behavior (update must verify+rollback; uninstall must
  delete only known app-owned paths).

## 7. JSON compatibility (§64)

Public `schema_version = "1.0"`. v0.x may add optional fields; renaming or
removing a field requires a schema bump + compatibility test.

## 8. Test commands (offline)

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo xtask check          # source-registry + acceptance-matrix + doc coverage
cargo xtask check-matrix   # acceptance-matrix structural validation
```

Coverage (M7): `cargo llvm-cov`. Supply chain (M7): `cargo deny check`.

## 9. Baseline commands (§57)

```bash
cargo xtask baseline            # functional/quality/perf baseline orchestration
cargo xtask static-release <b>  # musl/static-link verification
```

## 10. Source fixture policy (§45, §63)

Every enabled source needs ≥1 list fixture, ≥1 detail fixture (if it uses
detail), and ≥1 golden expectation. On a site redesign: reproduce → refresh the
sanitized fixture → update the adapter → targeted tests → bump
`last_verified` → baseline → commit. Never hack a selector against live HTML
without adding a fixture.

## 11. Update / uninstall risk (§34, §35)

Self-update verifies SHA-256 before replace, keeps a rollback copy, and never
deletes the working binary on failure. Uninstall deletes only known app-owned
paths; it never `rm -rf $HOME`, never follows arbitrary symlinks, and requires
explicit `--keep-data --yes` or `--purge --yes` when noninteractive. Development
binaries (`cargo run`) must not be silently deleted.

## 12. Things that require USER sign-off (§0.3)

Changing product positioning, deleting a major capability axis, changing
persisted-data semantics, changing the public JSON schema compatibility policy,
dropping pure-Rust CLI, introducing browser automation / JS runtime, introducing
a remote LLM/API as a runtime dependency, changing the self-update trust model,
or changing main-only development.

## 13. Completion discipline

Commit immediately after each verified work item. One commit = one semantic
intent. Read `git log` style before composing a message. Never commit secrets,
`.env`, tokens, or private data.
