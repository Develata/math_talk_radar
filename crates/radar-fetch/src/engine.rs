//! Async fetch engine (§15, §16, §32). Concurrency, timeout, robots, budget,
//! deadline, failure isolation.
use chrono::Utc;
use radar_core::{
    AdapterError, EventCandidate, FetchedDocument, SourceAdapter, SourceHealth, SourceStatus,
    config::SourceSpec,
};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use url::Url;

use crate::budget::RequestBudget;
use crate::client::{FetchClient, make_document};
use crate::error::FetchError;
use crate::fetch_policy::FetchPolicy;
use crate::policy::HttpPolicy;
use crate::retry::{is_transient_network_error, retry_for_status};
use crate::robots::{RobotsCache, RobotsRules};

pub struct SourceFetchResult {
    pub candidates: Vec<EventCandidate>,
    pub health: SourceHealth,
}

/// R9-H10: per-source upper bound on discovered stubs that proceed to
/// enrichment. Output caps (`--max-events`/`--max-talks`) only truncate the
/// final output; without this cap a single runaway source listing tens of
/// thousands of events would drive unbounded enrichment fetches, dedup,
/// scoring, and state writes before any output cap fires. The limit is
/// generous for legitimate calendars (the largest real source in the
/// registry lists ~600 events) while bounding worst-case resource use.
pub const MAX_STUBS_PER_SOURCE: usize = 2000;

pub fn past_deadline(deadline: Option<Instant>) -> bool {
    deadline.map(|d| Instant::now() >= d).unwrap_or(false)
}

fn remaining_time(deadline: Option<Instant>, default: std::time::Duration) -> std::time::Duration {
    deadline
        .map(|d| d.saturating_duration_since(Instant::now()))
        .unwrap_or(default)
}

/// Fetch robots.txt following redirects safely. Per RFC 9309 §2.3.1:
/// 4xx → allow-all, 5xx → disallow-all, https→http downgrade or
/// non-http(s) scheme → disallow-all. Cross-host/port redirects are FOLLOWED
/// (RFC 9309 §2.3.1.2 SHOULD follow cross-authority redirects), so the common
/// http→https / bare-domain→www robots.txt canonicalization does not silently
/// disallow-all. Body exceeding `max_response_body` → disallow-all
/// (conservative: an oversized policy could hide disallow rules).
///
/// B6: a robots.txt redirect to a host NOT in `allowed_hosts` is treated as
/// disallow-all. Without this, a malicious server could redirect robots.txt to
/// an attacker-controlled host returning an allow-all policy, bypassing the
/// real robots rules. When `allowed_hosts` is empty (no restriction), all
/// cross-host redirects are followed per the RFC.
async fn fetch_robots_txt(
    client: &FetchClient,
    start_url: Url,
    http_policy: &HttpPolicy,
    allowed_hosts: &[String],
    deadline: Option<Instant>,
    budget: &mut RequestBudget,
) -> Result<RobotsRules, FetchError> {
    if past_deadline(deadline) {
        return Err(FetchError::Timeout);
    }
    let mut current = start_url;
    for _ in 0..=http_policy.redirect_limit {
        if past_deadline(deadline) {
            return Err(FetchError::Timeout);
        }
        // H01: robots.txt fetches must consume budget and acquire the
        // per-host permit like any other request — otherwise the request
        // count is unbounded and per-host concurrency is not respected.
        if !budget.try_consume() {
            return Err(FetchError::BudgetExhausted);
        }
        let _host_permit = client.acquire_host_permit(&current).await;
        let timeout = remaining_time(deadline, http_policy.request_timeout);
        if timeout.is_zero() {
            return Err(FetchError::Timeout);
        }
        let mut resp = client
            .handle()
            .get(current.as_str())
            .timeout(timeout)
            .send()
            .await
            .map_err(FetchError::from)?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let new_url = match current.join(location) {
                Ok(u) => u,
                Err(_) => return Ok(RobotsRules::disallow_all()),
            };
            // Only block on downgrade or unsafe scheme; cross-host/port is
            // followed per RFC 9309 §2.3.1.2.
            if current.scheme() == "https" && new_url.scheme() == "http" {
                return Ok(RobotsRules::disallow_all());
            }
            if !matches!(new_url.scheme(), "http" | "https") {
                return Ok(RobotsRules::disallow_all());
            }
            // B6: a robots.txt redirect to a host outside `allowed_hosts` is
            // suspicious — disallow-all rather than following it.
            if !allowed_hosts.is_empty()
                && let Some(new_host) = new_url.host_str()
                && !allowed_hosts.iter().any(|a| a == new_host)
            {
                return Ok(RobotsRules::disallow_all());
            }
            current = new_url;
            continue;
        }
        if resp.status().is_success() {
            let mut buf = Vec::new();
            let mut capped = false;
            while let Some(chunk) = resp.chunk().await.map_err(FetchError::from)? {
                if buf.len() + chunk.len() > http_policy.max_response_body {
                    capped = true;
                    break;
                }
                buf.extend_from_slice(&chunk);
            }
            if capped {
                // Conservative: an oversized robots.txt could be hiding
                // disallow rules. Disallow all rather than risk crawling
                // without having seen the full policy (§47 philosophy).
                return Ok(RobotsRules::disallow_all());
            }
            let body = String::from_utf8_lossy(&buf);
            return Ok(crate::robots::parse_robots(&body));
        }
        if (400..500).contains(&status) {
            return Ok(RobotsRules::default());
        }
        return Ok(RobotsRules::disallow_all());
    }
    Ok(RobotsRules::disallow_all())
}

/// Fetch (or reuse cached) robots rules for `url`'s host and verify the URL
/// is allowed. Called before the initial request and after every redirect
/// that lands on a new host (H1: redirect must not bypass robots).
async fn check_robots(
    client: &FetchClient,
    url: &Url,
    http_policy: &HttpPolicy,
    allowed_hosts: &[String],
    robots: &RobotsCache,
    deadline: Option<Instant>,
    budget: &mut RequestBudget,
) -> Result<(), FetchError> {
    let owned_url = url.clone();
    let client_clone = client.clone();
    let hp = *http_policy;
    let ah = allowed_hosts.to_vec();
    // B6-1: include a sorted allowlist hash in the cache key so sources
    // with different host policies get separate robots cache entries.
    let mut sorted_hosts = allowed_hosts.to_vec();
    sorted_hosts.sort();
    sorted_hosts.dedup();
    let allowlist_key = sorted_hosts.join(",");
    let rules = robots
        .get_or_init(url, &allowlist_key, move || async move {
            match robots_url_for(&owned_url) {
                Some(ru) => fetch_robots_txt(&client_clone, ru, &hp, &ah, deadline, budget).await,
                None => Ok(RobotsRules::default()),
            }
        })
        .await?;
    if !rules.is_allowed(url) {
        return Err(FetchError::RobotsDenied { url: url.clone() });
    }
    Ok(())
}

/// Fetch a single URL with manual redirect loop, body cap, retry.
pub async fn fetch_one(
    client: &FetchClient,
    url: &Url,
    policy: &FetchPolicy,
    http_policy: &HttpPolicy,
    budget: &mut RequestBudget,
    deadline: Option<Instant>,
    robots: &RobotsCache,
) -> Result<FetchedDocument, FetchError> {
    if past_deadline(deadline) {
        return Err(FetchError::Timeout);
    }

    // §16: verify the initial URL host before any request leaves.
    if !policy.is_host_allowed(url) {
        return Err(FetchError::RedirectDisallowed);
    }

    // Robots check (initial URL). check_robots caches rules per host; the
    // redirect loop below re-checks when a hop lands on a new host.
    check_robots(
        client,
        url,
        http_policy,
        &policy.allowed_hosts,
        robots,
        deadline,
        budget,
    )
    .await?;

    // Manual redirect loop (Oracle #7). Network errors (B1, §15: connection
    // reset, transient) and status retries (§15: 408, 429, 5xx) share a single
    // retry budget bounded by `max_retry`. After any retry we loop back to the
    // top so a 3xx on the retried response is followed as a redirect (W5).
    let mut current_url = url.clone();
    let mut last_host = url.host_str().map(|h| h.to_string());
    let mut last_port = url.port();
    let mut last_scheme = url.scheme().to_string();
    let mut hops = 0;
    let max_hops = http_policy.redirect_limit;
    let mut retries_used: u32 = 0;

    loop {
        // FETCH-7: check the deadline before acquiring the per-host permit.
        // A long permit wait can let the deadline pass; recomputing after
        // the await ensures the send uses a fresh timeout.
        if past_deadline(deadline) {
            return Err(FetchError::Timeout);
        }
        // H01: every HTTP request — initial, redirect hop, and retry —
        // consumes budget. Without this, redirect/retry chains exceed the
        // configured request_budget.
        if !budget.try_consume() {
            return Err(FetchError::BudgetExhausted);
        }
        let _host_permit = client.acquire_host_permit(&current_url).await;
        let timeout = remaining_time(deadline, http_policy.request_timeout);
        if timeout.is_zero() {
            return Err(FetchError::Timeout);
        }

        let mut resp = match client
            .handle()
            .get(current_url.as_str())
            .timeout(timeout)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) if is_transient_network_error(&e) && retries_used < http_policy.max_retry => {
                retries_used += 1;
                continue;
            }
            Err(e) => return Err(FetchError::from(e)),
        };

        let status = resp.status().as_u16();

        if (300..400).contains(&status) {
            hops += 1;
            if hops > max_hops {
                return Err(FetchError::RedirectDisallowed);
            }
            let location = match resp.headers().get(reqwest::header::LOCATION) {
                Some(loc) => loc.to_str().unwrap_or("").to_string(),
                None => return Err(FetchError::RedirectDisallowed),
            };
            let new_url = current_url
                .join(&location)
                .map_err(|_| FetchError::RedirectDisallowed)?;
            if !policy.is_host_allowed(&new_url) {
                return Err(FetchError::RedirectDisallowed);
            }
            if current_url.scheme() == "https" && new_url.scheme() == "http" {
                return Err(FetchError::RedirectDisallowed);
            }
            if !matches!(new_url.scheme(), "http" | "https") {
                return Err(FetchError::RedirectDisallowed);
            }
            // H1: a redirect to a new origin (host, port, or scheme) must
            // re-check robots for that origin before the next request leaves.
            // Scheme matters because robots.txt is per-scheme (RFC 9309
            // §2.3.1.1): http://example.org/robots.txt and
            // https://example.org/robots.txt are separate documents. B03: a
            // scheme-only upgrade http→https on the same host previously
            // skipped re-check, allowing a site with strict https robots and
            // lax http robots to bypass the https policy.
            let new_host = new_url.host_str().map(|h| h.to_string());
            let new_scheme = new_url.scheme().to_string();
            if new_host != last_host || new_url.port() != last_port || new_scheme != last_scheme {
                check_robots(
                    client,
                    &new_url,
                    http_policy,
                    &policy.allowed_hosts,
                    robots,
                    deadline,
                    budget,
                )
                .await?;
                last_host = new_host;
                last_port = new_url.port();
                last_scheme = new_scheme;
            }
            current_url = new_url;
            continue;
        }

        let retry = retry_for_status(
            status,
            resp.headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| crate::retry::parse_retry_after(s, Utc::now())),
        );
        if let crate::retry::RetryDecision::Retry { after } = retry {
            if retries_used < http_policy.max_retry {
                retries_used += 1;
                let delay = match after {
                    Some(d) => {
                        std::cmp::min(d, remaining_time(deadline, http_policy.request_timeout))
                    }
                    None => {
                        // FS-4: no Retry-After header → exponential backoff so
                        // the server is not hammered. Base 500 ms, doubled per
                        // retry, capped by the remaining time to the deadline.
                        let retry_index = retries_used.saturating_sub(1);
                        let backoff_ms = 500u64.saturating_mul(1u64 << retry_index.min(20));
                        std::cmp::min(
                            std::time::Duration::from_millis(backoff_ms),
                            remaining_time(deadline, http_policy.request_timeout),
                        )
                    }
                };
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                continue;
            } else {
                return Err(FetchError::HttpError { status });
            }
        }

        if !resp.status().is_success() {
            return Err(FetchError::HttpError { status });
        }

        if let Some(len) = resp.content_length()
            && (len as usize) > http_policy.max_response_body
        {
            return Err(FetchError::BodyTooLarge {
                limit: http_policy.max_response_body,
            });
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let body = {
            let mut buf = Vec::new();
            while let Some(chunk) = resp.chunk().await.map_err(FetchError::from)? {
                if buf.len() + chunk.len() > http_policy.max_response_body {
                    return Err(FetchError::BodyTooLarge {
                        limit: http_policy.max_response_body,
                    });
                }
                buf.extend_from_slice(&chunk);
            }
            buf
        };

        let final_url = current_url.clone();
        return Ok(make_document(
            url.clone(),
            final_url,
            status,
            content_type,
            body,
            Utc::now(),
        ));
    }
}

fn robots_url_for(url: &Url) -> Option<Url> {
    url.host_str()?;
    let mut robots = url.clone();
    robots.set_path("/robots.txt");
    robots.set_query(None);
    robots.set_fragment(None);
    Some(robots)
}

/// Fetch a single source: entrypoint -> discover -> enrich (Oracle #4: inline enrich).
pub async fn fetch_source(
    client: &FetchClient,
    source: &SourceSpec,
    adapter: &dyn SourceAdapter,
    robots: &RobotsCache,
    deadline: Option<Instant>,
) -> SourceFetchResult {
    let start = Instant::now();

    if !source.enabled {
        return SourceFetchResult {
            candidates: vec![],
            health: SourceHealth {
                source: source.id.clone(),
                status: SourceStatus::Disabled,
                duration_ms: 0,
                requests: 0,
                events: 0,
                recorded_at: None,
            },
        };
    }

    let policy = FetchPolicy::from(source);
    let mut budget = RequestBudget {
        max_depth: source.max_depth,
        request_budget: source.request_budget,
        remaining: source.request_budget,
    };
    let http_policy = client.policy();

    let entrypoint = match &source.entrypoint {
        Some(u) => u.clone(),
        None => {
            return SourceFetchResult {
                candidates: vec![],
                health: SourceHealth {
                    source: source.id.clone(),
                    status: SourceStatus::ParseError,
                    duration_ms: start.elapsed().as_millis() as u64,
                    requests: 0,
                    events: 0,
                    recorded_at: None,
                },
            };
        }
    };

    let doc = match fetch_one(
        client,
        &entrypoint,
        &policy,
        &http_policy,
        &mut budget,
        deadline,
        robots,
    )
    .await
    {
        Ok(d) => d,
        Err(e) => {
            return SourceFetchResult {
                candidates: vec![],
                health: SourceHealth {
                    source: source.id.clone(),
                    status: e.to_source_status(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    requests: source.request_budget - budget.remaining,
                    events: 0,
                    recorded_at: None,
                },
            };
        }
    };

    let mut stubs = match adapter.discover(&doc, source) {
        Ok(s) => s,
        Err(AdapterError::Parse { .. })
        | Err(AdapterError::DynamicUnsupported(_))
        | Err(AdapterError::BudgetExhausted(_)) => {
            return SourceFetchResult {
                candidates: vec![],
                health: SourceHealth {
                    source: source.id.clone(),
                    status: SourceStatus::ParseError,
                    duration_ms: start.elapsed().as_millis() as u64,
                    requests: source.request_budget - budget.remaining,
                    events: 0,
                    recorded_at: None,
                },
            };
        }
    };

    // R9-H10: cap stubs before enrichment so a runaway source cannot drive
    // unbounded downstream work. Truncation is reflected in the source
    // status (Partial) so the operator sees the source was clipped.
    let stubs_truncated = stubs.len() > MAX_STUBS_PER_SOURCE;
    if stubs_truncated {
        stubs.truncate(MAX_STUBS_PER_SOURCE);
    }

    let mut candidates = Vec::new();
    let mut enrichment_failures = 0u32;
    for stub in stubs {
        let plans = adapter.plan_enrichment(&stub, source);
        let mut docs = Vec::new();
        for plan in &plans {
            if plan.depth > budget.max_depth {
                continue;
            }
            match fetch_one(
                client,
                &plan.url,
                &policy,
                &http_policy,
                &mut budget,
                deadline,
                robots,
            )
            .await
            {
                Ok(d) => docs.push(d),
                Err(_) => {
                    enrichment_failures += 1;
                    continue;
                }
            }
        }
        // ADAP M-2 / H6: the entrypoint-document fallback must fire ONLY when
        // plan_enrichment emitted zero fetches (e.g. a JSON-LD Event whose url
        // equals the listing page, or a YouTube RSS stub that needs no detail
        // fetch). When plans were emitted but all failed, pass `docs` as-is
        // (empty) and let `enrich` decide.
        let docs_ref: &[FetchedDocument] = if plans.is_empty() {
            std::slice::from_ref(&doc)
        } else {
            &docs
        };
        match adapter.enrich(stub, docs_ref, source) {
            Ok(candidate) => candidates.push(candidate),
            Err(_) => {
                enrichment_failures += 1;
                continue;
            }
        }
    }

    let events = candidates.len() as u32;
    let status = if enrichment_failures > 0 || stubs_truncated {
        SourceStatus::Partial
    } else {
        SourceStatus::Ok
    };
    SourceFetchResult {
        candidates,
        health: SourceHealth {
            source: source.id.clone(),
            status,
            duration_ms: start.elapsed().as_millis() as u64,
            requests: source.request_budget - budget.remaining,
            events,
            recorded_at: None,
        },
    }
}

/// Fetch all sources concurrently (Oracle #1, #8: failure isolation).
pub async fn fetch_all(
    client: &FetchClient,
    sources: &[SourceSpec],
    deadline: Option<Instant>,
    adapter_factory: impl Fn(&SourceSpec) -> Box<dyn SourceAdapter>,
) -> Vec<SourceFetchResult> {
    let global_sem = Arc::new(Semaphore::new(client.policy().global_concurrency));
    let robots = Arc::new(RobotsCache::new());

    let mut sorted: Vec<&SourceSpec> = sources.iter().collect();
    sorted.sort_by_key(|s| &s.id);
    let source_ids: Vec<String> = sorted.iter().map(|s| s.id.clone()).collect();

    let mut join_set = tokio::task::JoinSet::new();
    for source in sorted {
        let client = client.clone();
        let source = source.clone();
        let adapter = adapter_factory(&source);
        let global_sem = global_sem.clone();
        let robots = robots.clone();

        join_set.spawn(async move {
            // Global permit held for the entire source fetch. Per-host
            // limiting is applied per-request inside fetch_one (FS-2),
            // keyed on the actual request URL so enrichment to other
            // hosts is not gated by the entrypoint host.
            let _global_permit = global_sem.acquire_owned().await.ok();

            fetch_source(&client, &source, adapter.as_ref(), &robots, deadline).await
        });
    }

    let mut results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(r) => results.push(r),
            Err(e) => {
                if e.is_panic() {
                    // Oracle #8: never propagate panics. The panicked source
                    // produced no SourceFetchResult; a synthetic entry is
                    // patched below so it does not vanish from source_health.
                }
            }
        }
    }

    // FS-3: a panicked source task produces no SourceFetchResult. Patch a
    // synthetic entry for any source id missing from the results so §32
    // partial-failure reporting stays complete.
    for id in &source_ids {
        if !results.iter().any(|r| &r.health.source == id) {
            results.push(SourceFetchResult {
                candidates: vec![],
                health: SourceHealth {
                    source: id.clone(),
                    status: SourceStatus::ParseError,
                    duration_ms: 0,
                    requests: 0,
                    events: 0,
                    recorded_at: None,
                },
            });
        }
    }

    // Sort by source id for stable ordering (Oracle #6)
    results.sort_by_key(|r| r.health.source.clone());
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn past_deadline_none_is_false() {
        assert!(!past_deadline(None));
    }

    #[test]
    fn past_deadline_past_is_true() {
        let past = Instant::now() - std::time::Duration::from_secs(1);
        assert!(past_deadline(Some(past)));
    }

    #[test]
    fn fetch_policy_from_source() {
        let spec = SourceSpec {
            id: "test".into(),
            name: "Test".into(),
            tier: radar_core::config::SourceTier::A,
            kind: radar_core::config::SourceKind::RssFeed,
            adapter: radar_core::config::AdapterKind::Rss,
            entrypoint: Some(Url::parse("https://example.com/feed.xml").unwrap()),
            allowed_hosts: vec![],
            max_depth: 2,
            request_budget: 20,
            media_strategy: None,
            dynamic: false,
            enabled: true,
            fixture: None,
            selectors: None,
        };
        let policy = FetchPolicy::from(&spec);
        assert_eq!(policy.allowed_hosts, vec!["example.com".to_string()]);
    }

    // R9-B01: robots_url_for must strip the query string so a source
    // entrypoint like /events?next=5 fetches /robots.txt (not /robots.txt?next=5).
    #[test]
    fn robots_url_for_strips_query() {
        let url = Url::parse("https://example.org/events?next=5").unwrap();
        let robots = robots_url_for(&url).unwrap();
        assert_eq!(robots.as_str(), "https://example.org/robots.txt");
        assert!(robots.query().is_none());
    }

    #[test]
    fn robots_url_for_strips_fragment() {
        let url = Url::parse("https://example.org/events#section").unwrap();
        let robots = robots_url_for(&url).unwrap();
        assert_eq!(robots.as_str(), "https://example.org/robots.txt");
    }
}
