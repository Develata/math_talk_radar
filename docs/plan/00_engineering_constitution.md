# 00 — Engineering Constitution

> Status: M0 skeleton. Authoritative. §61 minimum rules.

This document is the top of the engineering design truth source. Everything in
`docs/plan/` and the codebase must be consistent with it.

## Authority order (§0.2)

```
USER current explicit instruction
  → 00_engineering_constitution.md
  → docs/plan/*.md
  → docs/reference/*.md
  → docs/acceptance-cases/*.md
  → docs/registry/*
  → code
  → docs/report/*
```

Plans are truth; code is a projection; acceptance cases are proof; reports are
evidence only. If implementation shows a plan assumption is wrong, write an ADR
first, then update the plan — never silently weaken the contract.

## Minimum rules (§61)

- correctness > convenience
- architecture boundary > local shortcut
- determinism > heuristic magic
- evidence > inference
- structured data > generic scraping
- failure isolation
- bounded resource usage
- no silent destructive behavior
- testability without internet

Core state, configuration, and the output schema each have a **single source of
truth**.

## Process (§0.1, §0.3)

- main-only development; one atomic commit per verified milestone (M0–M8).
- Work order: read AGENTS.md → constitution → relevant plan → acceptance cases →
  implement → tests → review → baseline → commit.

## Acceptance cases

- DOC-001 — every plan has an acceptance mapping (`cargo xtask check-matrix`).
- DOC-002 — every hard case has automation (`cargo xtask check-matrix`).
