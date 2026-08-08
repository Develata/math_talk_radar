# Implementation Status

> Evidence only — non-authoritative (§0.2). Updated per milestone.

## Current milestone: M1 — Core Domain

- [x] Date parser: `parse_date` (ISO, same-month range, cross-month range, US
      format, DMY single) + `interval_overlap`. 11 unit tests (DATE-001..005).
- [x] Normalization pipeline: `normalize_name` (NFC + case + whitespace) +
      `word_boundaries` (Unicode segmentation). 8 unit tests.
- [x] Scholar matcher: `match_scholars` with `MatchContext` role protection
      (§6.2). Ambiguous-surname filtering (Li/Wang/Tao/Yau/Gross/Wei/Wu). 7 unit
      tests (PER-001..003).
- [x] Topic matcher: `match_topics` with single-word word-boundary vs multi-word
      substring matching. 8 unit tests (TOP-001).
- [x] Ranking composer: `score_event` with 6 signal components (§26) +
      `InterestWeights`. 8 unit tests (RANK-001..003).
- [x] Golden datasets: 56 date cases, 66 people cases, 25 ranking cases (147
      total). §47 metrics: date accuracy 1.000, scholar precision 1.000, recall
      1.000, role-protection FP 0. `cargo test --test golden` passes.
- [x] M1 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, `cargo test --workspace`, `cargo xtask
      check`, `cargo xtask check-matrix` — all pass.
- [x] 12 acceptance cases flipped to `pass`: DATE-001..005, PER-001..003,
      TOP-001, RANK-001..003.
- [x] One multilingual alias added to `config/scholars.toml` (陶哲轩 →
      terence-tao) for PER-002.

## Next: M2 — Source Adapters & Fetching

RSS/ICS/JSON-LD/HTML adapters in `radar-adapters`, fetch coordinator in
`radar-fetch`, mock-server HTTP tests, talk extraction (TALK-001).
