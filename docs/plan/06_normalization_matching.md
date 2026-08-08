# 06 — Normalization & Matching

> Status: M0 skeleton. Authoritative for normalization, people/topic matching,
> dedup. §6.2, §7, §25, §47. Implemented in `radar-core`.

## Name matching (§6.2)

Pipeline: Unicode normalization → case normalization → explicit alias → Unicode
word boundary → context/field role. Short/ambiguous surnames (Li, Wang, Tao, Yau,
Gross, ...) require a structured person field or very strong name-in-context to
produce an entity match. Title mentions ("Gross-Zagier Formula", "Deligne
periods", "Ahlfors Lecture Series") must NOT yield `speaker`.

## Topics (§7)

Canonical topic + aliases + phrases + optional user interest weights. User
interests alter ranking only; they never delete other important events.

## Dedup (§25)

Conservative deterministic dedup. Priority: canonical URL → source-declared
canonical ID → normalized title+date+organizer → normalized title+date+location.
Prefer keeping a suspected duplicate over merging two distinct events. Fuzzy
semantic dedup deferred.

## Accuracy baselines (§47)

- Date parser: labeled baseline accuracy ≥ 98%.
- Scholar entity matching: precision ≥ 95%, recall ≥ 95%.
- Role protection: negative examples (Deligne periods, Gross-Zagier formula,
  Ahlfors Lecture Series) mislabeled as `speaker` = 0.
- Event fixture discovery: recall ≥ 95%, navigation/noise precision ≥ 90%.
- Media fixture discovery: recall ≥ 95%.
- Conservative dedup: precision = 100% on labeled baseline, recall ≥ 90%. A
  wrong merge of two distinct events is a release blocker.

## Acceptance cases

- PER-001 — scholar alias (golden).
- PER-002 — multilingual alias (golden).
- PER-003 — concept name not speaker (golden).
- TOP-001 — topic alias matching (golden).
- DEDUP-001 — identical event merge (golden).
- DEDUP-002 — distinct event not merge (golden).
