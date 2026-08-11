# 01 — Product Scope

> Status: M0 skeleton. Authoritative for positioning and coverage. §1–4, §18,
> §70, §71, §76.

## Product definition (§1.1)

`math_talk_radar` is a **radar for discovering public mathematics conferences,
talks, lecture series, recordings, slides, and related resources**. It is NOT a
generic scraper that hunts for famous surnames. It turns dispersed structured
and semi-structured data into stable, explainable records.

## User goals (§2)

Discover high-value activities → discover talks/speakers/abstracts → discover
public livestream/recordings/slides → track new material over time → give
downstream AI enough context to judge and summarize.

## Core principles (§3)

- **P-1 Content first**: scholar prestige is a ranking signal, not a hard filter.
- **P-2 People have roles**: a name in text is at most `title_mention`/`unknown`
  unless structured evidence exists.
- **P-3 Conference ≠ Talk ≠ Recording**: model `EventSeries → Event → Talk →
  MediaResource`.
- **P-4 Evidence first**: preserve evidence for important inferences.
- **P-5 Structured source first**: JSON/API → RSS/Atom → ICS → JSON-LD →
  site-specific HTML → generic HTML fallback.

## v0.1 scope (§4.1) and non-goals (§4.2)

In scope: discovery, detail enrichment, talks, people roles, topics, scholar
ranking, date ranges, media discovery, cross-source dedup, source health, local
state, change detection, JSON output, self-update, uninstall, doctor, CI, static
release, acceptance matrix, baseline, ≥20 source audit, ≥10 enabled fixture
sources, ≥3 media/recording paths.

Non-goals: LLM, embeddings, vector DB, browser automation, JS runtime, video
download/transcription, Cloudflare bypass, login sites, paywalled content,
CAPTCHA, deep crawling, generic search engines, theorem proving.

## Source coverage baseline (§18)

Audited ≥ 20, enabled ≥ 10, structured adapter kinds ≥ 2 (v0.1; restore to
≥ 3 when a qualified JSON-LD/ICS source is audited — see ADR-0007),
media/recording sources ≥ 3. Audit the real current entrypoint before writing
a URL — do not mechanically reuse possibly-stale URLs.

## Recommended scan defaults (§70)

`mode=both`, `before=30`, `after=180`, `jobs=8`, `max_events=100`,
`max_talks=300`, `media_followup_days=180`.

## Downstream AI contract (§71, §76)

The radar emits evidence (events, topics, abstracts, speakers, talk titles,
media, source tier, ranking reasons). Downstream agents produce interpretation.
README must NOT advertise "AI-powered research assistant" — v0.1 has no AI.

## Acceptance cases

- LIVE-001 — ≥20 audited sources.
- LIVE-002 — ≥10 enabled fixture-backed sources.
- LIVE-003 — live source health ratio (advisory).
