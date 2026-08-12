//! Async fetch engine (§15, §16, §32). Concurrency, timeout, robots, budget,
//! deadline, failure isolation.
use chrono::Utc;
use radar_core::{
    AdapterError, EventCandidate, FetchedDocument, SourceAdapter, SourceHealth, SourceStatus,
    config::SourceSpec,
};
use std::collections::HashMap;
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

pub fn past_deadline(deadline: Option<Instant>) -> bool {
    deadline.map(|d| Instant::now() >= d).unwrap_or(false)
}

fn remaining_time(deadline: Option<Instant>, default: std::time::Duration) -> std::time::Duration {
    deadline
        .map(|d| d.saturating_duration_since(Instant::now()))
        .unwrap_or(default)
}

/// Fetch robots.txt following same-host redirects safely. Per RFC 9309
/// §2.3.1: 4xx → allow-all, 5xx → disallow-all, cross-host/unsafe-scheme
/// redirect → disallow-all. Body is capped at `max_response_body`.
async fn fetch_robots_txt(
    client: &FetchClient,
    start_url: Url,
    http_policy: &HttpPolicy,
) -> Result<RobotsRules, FetchError> {
    let mut current = start_url;
    for _ in 0..=http_policy.redirect_limit {
        let mut resp = client
            .handle()
            .get(current.as_str())
            .timeout(http_policy.request_timeout)
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
            if current.scheme() == "https" && new_url.scheme() == "http" {
                return Ok(RobotsRules::disallow_all());
            }
            if !matches!(new_url.scheme(), "http" | "https") {
                return Ok(RobotsRules::disallow_all());
            }
            if new_url.host_str() != current.host_str() || new_url.port() != current.port() {
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
                return Ok(RobotsRules::default());
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
    robots: &RobotsCache,
) -> Result<(), FetchError> {
    let owned_url = url.clone();
    let client_clone = client.clone();
    let hp = *http_policy;
    let rules = robots
        .get_or_init(url, move || async move {
            match robots_url_for(&owned_url) {
                Some(ru) => fetch_robots_txt(&client_clone, ru, &hp).await,
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
    if !budget.try_consume() {
        return Err(FetchError::BudgetExhausted);
    }

    // §16: verify the initial URL host before any request leaves.
    if !policy.is_host_allowed(url) {
        return Err(FetchError::RedirectDisallowed);
    }

    // Robots check (initial URL). check_robots caches rules per host; the
    // redirect loop below re-checks when a hop lands on a new host.
    check_robots(client, url, http_policy, robots).await?;

    // Manual redirect loop (Oracle #7). Network errors (B1, §15: connection
    // reset, transient) and status retries (§15: 408, 429, 5xx) share a single
    // retry budget bounded by `max_retry`. After any retry we loop back to the
    // top so a 3xx on the retried response is followed as a redirect (W5).
    let mut current_url = url.clone();
    let mut last_host = url.host_str().map(|h| h.to_string());
    let mut last_port = url.port();
    let mut hops = 0;
    let max_hops = http_policy.redirect_limit;
    let mut retries_used: u32 = 0;

    loop {
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
            // H1: a redirect to a new host must re-check robots for that host
            // before the next request leaves. Same-host redirects reuse the
            // cached rules.
            let new_host = new_url.host_str().map(|h| h.to_string());
            if new_host != last_host || new_url.port() != last_port {
                check_robots(client, &new_url, http_policy, robots).await?;
                last_host = new_host;
                last_port = new_url.port();
            }
            current_url = new_url;
            continue;
        }

        let retry = retry_for_status(
            status,
            resp.headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok().map(std::time::Duration::from_secs)),
        );
        if let crate::retry::RetryDecision::Retry { after } = retry {
            if retries_used < http_policy.max_retry {
                retries_used += 1;
                if let Some(delay) = after {
                    let capped =
                        std::cmp::min(delay, remaining_time(deadline, http_policy.request_timeout));
                    if !capped.is_zero() {
                        tokio::time::sleep(capped).await;
                    }
                }
                continue;
            } else {
                return Err(FetchError::HttpError { status });
            }
        }

        let final_status = resp.status().as_u16();
        if !resp.status().is_success() {
            return Err(FetchError::HttpError {
                status: final_status,
            });
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
            final_status,
            content_type,
            body,
            Utc::now(),
        ));
    }
}

fn robots_url_for(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let scheme = url.scheme();
    let port = match url.port() {
        Some(p) => format!(":{p}"),
        None => String::new(),
    };
    Url::parse(&format!("{scheme}://{host}{port}/robots.txt")).ok()
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
                },
            };
        }
    };

    let stubs = match adapter.discover(&doc, source) {
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
                },
            };
        }
    };

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
        match adapter.enrich(stub, &docs, source) {
            Ok(candidate) => candidates.push(candidate),
            Err(_) => {
                enrichment_failures += 1;
                continue;
            }
        }
    }

    let events = candidates.len() as u32;
    let status = if enrichment_failures > 0 {
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
    let host_sems = Arc::new(tokio::sync::Mutex::new(
        HashMap::<String, Arc<Semaphore>>::new(),
    ));
    let robots = Arc::new(RobotsCache::new());
    let per_host = client.policy().per_host_concurrency;

    let mut sorted: Vec<&SourceSpec> = sources.iter().collect();
    sorted.sort_by_key(|s| &s.id);

    let mut join_set = tokio::task::JoinSet::new();
    for source in sorted {
        let client = client.clone();
        let source = source.clone();
        let adapter = adapter_factory(&source);
        let global_sem = global_sem.clone();
        let host_sems = host_sems.clone();
        let robots = robots.clone();

        join_set.spawn(async move {
            // Acquire per-host permit first, then global. Ordering matters:
            // global-first creates a convoy — a task blocked on a saturated
            // host holds a global slot it cannot use, starving tasks on other
            // hosts that could proceed. Per-host-first ensures a task only
            // holds a global slot when it is ready to fetch.
            let host = source
                .entrypoint
                .as_ref()
                .and_then(|u| u.host_str())
                .unwrap_or("")
                .to_string();
            let host_sem = {
                let mut map = host_sems.lock().await;
                map.entry(host.clone())
                    .or_insert_with(|| Arc::new(Semaphore::new(per_host)))
                    .clone()
            };
            let _host_permit = host_sem.acquire_owned().await.ok();

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
                    // Oracle #8: never propagate panics
                }
                // fetch_source handles its own errors and returns a
                // SourceFetchResult; a panic here is swallowed so one bad
                // source cannot take down the whole scan.
            }
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
}
