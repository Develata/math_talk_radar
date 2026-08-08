# 05 — Fetching Reliability

> Status: M0 skeleton. Authoritative for HTTP. §15, §16, §32, §48. Implemented
> in `radar-fetch`.

## Defaults (§15)

```
global concurrency       = 8
per-host concurrency     = 2
connect timeout          = 5s
request timeout          = 15s
global scan deadline     = 30s
redirect limit           = 5
max retry                = 1
max response body        = 4 MiB
```

## Retry policy (§15)

Retry only: connection reset, transient network failure, 408, 429, 5xx. Never
retry: 400, 401, 403, 404, 410, robots denied, parse failure. 429 honors
`Retry-After` but never breaches the global deadline.

## HTTP security (§16)

HTTPS preferred; host must be in the source registry; re-check policy after
redirect; no cookies; no auth; response bodies not persisted beyond the bounded
`FetchedDocument`; no sensitive header logging. UA:
`math_talk_radar/<version> (+public-repository)`. `respect_robots = true`;
no robots bypass is ever provided.

## Scan deadline (§48)

Real network global default deadline = 30s. On timeout, return completed results
rather than waiting indefinitely.

## Failure isolation (§32)

30% source failure → still exit 0. All enabled sources failing → exit 4.

## Acceptance cases

- HTTP-001 — timeout isolation (mock server).
- HTTP-002 — retry transient once (mock server).
- HTTP-003 — 404 no retry (mock server).
- HTTP-004 — partial source failure exit 0 (integration).
- HTTP-005 — zero usable source exit 4 (integration).
- REL-001 — 30% source failure isolation (fault injection).
- REL-002 — global deadline (mock server).
- REL-003 — stable deterministic IDs (golden).
