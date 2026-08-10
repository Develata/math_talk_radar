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
use crate::robots::RobotsCache;

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

    // Robots check (clone to owned so the fetch closure is 'static + Send).
    let owned_url = url.clone();
    let client_clone = client.clone();
    let robots_body_cap = http_policy.max_response_body;
    let rules = robots
        .get_or_init(url, move || async move {
            let robots_url = robots_url_for(&owned_url);
            match robots_url {
                Some(ru) => {
                    let mut resp = client_clone
                        .handle()
                        .get(ru.as_str())
                        .send()
                        .await
                        .map_err(FetchError::from)?;
                    if resp.status().is_success() {
                        // §66: cap robots.txt body — a malicious host could
                        // otherwise exhaust the RSS budget with an enormous
                        // robots.txt. On cap breach, return empty rules
                        // (allow-all) so the host cannot DoS the crawl.
                        let mut buf = Vec::new();
                        let mut capped = false;
                        while let Some(chunk) = resp.chunk().await.map_err(FetchError::from)? {
                            if buf.len() + chunk.len() > robots_body_cap {
                                capped = true;
                                break;
                            }
                            buf.extend_from_slice(&chunk);
                        }
                        if capped {
                            return Ok(crate::robots::RobotsRules::default());
                        }
                        let body = String::from_utf8_lossy(&buf);
                        Ok(crate::robots::parse_robots(&body))
                    } else {
                        Ok(crate::robots::RobotsRules::default())
                    }
                }
                None => Ok(crate::robots::RobotsRules::default()),
            }
        })
        .await?;
    if !rules.is_allowed(url) {
        return Err(FetchError::RobotsDenied { url: url.clone() });
    }

    // Manual redirect loop (Oracle #7). Network errors (B1, §15: connection
    // reset, transient) and status retries (§15: 408, 429, 5xx) share a single
    // retry budget bounded by `max_retry`. After any retry we loop back to the
    // top so a 3xx on the retried response is followed as a redirect (W5).
    let mut current_url = url.clone();
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
            // Acquire global permit
            let _global_permit = global_sem.acquire_owned().await.ok();

            // Acquire per-host permit (Oracle #1: outside lock)
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
