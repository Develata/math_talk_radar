# 08 — Ranking

> Status: M0 skeleton. Authoritative for scoring. §26, §26.1, §47. Implemented
> in `radar-core`.

## Signal caps (§26)

| Signal | Max |
|---|---:|
| topic relevance | 30 |
| public recording available | 25 |
| livestream / explicit recording plan | 15 |
| institution/event tier | 10 |
| important scholar actually participating | 10 |
| program/abstract completeness | 10 |

Default score 0–100. A title-only scholar mention ("Deligne ...", "Gross-Zagier
...") does NOT receive the people component.

## Explainability (§26.1)

Output must include `score_components` and `rank_reasons`, e.g.:
```json
"score_components": { "topic": 22, "media": 25, "access": 10, "source_tier": 10, "people": 8, "completeness": 10 },
"rank_reasons": ["public_recording_available", "major_research_institute", "matched_topic: arithmetic_geometry"]
```

## Accuracy baseline (§47)

Ranking cases ≥ 20 in the golden dataset.

## Acceptance cases

- RANK-001 — topic score (golden).
- RANK-002 — recording score (golden).
- RANK-003 — title-only scholar no boost (golden).
