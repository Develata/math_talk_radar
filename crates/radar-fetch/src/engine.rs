//! Async fetch engine (§15, §16, §32). Concurrency, timeout, robots, budget,
//! deadline, failure isolation.
use radar_core::{
    AdapterError, EventCandidate, SourceAdapter, SourceHealth, SourceStatus, config::SourceSpec,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
#[cfg(test)]
use url::Url;

use crate::budget::RequestBudget;
use crate::client::FetchClient;
use crate::fetch_policy::FetchPolicy;
pub use crate::http::{fetch_one, past_deadline};
use crate::robots::RobotsCache;

pub struct SourceFetchResult {
    pub candidates: Vec<EventCandidate>,
    pub health: SourceHealth,
}

pub use radar_core::adapter::MAX_STUBS_PER_SOURCE;

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
        // Inline enrichment may perform no await at all. Check between bounded
        // parser calls so CPU-only sources also respect the scan deadline.
        if past_deadline(deadline) {
            enrichment_failures += 1;
            break;
        }
        let plans = adapter.plan_enrichment(&stub, source);
        let mut docs = Vec::new();
        for plan in &plans {
            if plan.depth > budget.max_depth {
                enrichment_failures += 1;
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
        // equals the listing page). The previous `if docs.is_empty()` conflated
        // that case with "plans existed but every detail fetch failed" — in
        // the latter, substituting the list page as a detail document fed the
        // adapter stale/wrong data and silently masked a total enrichment
        // failure. When plans were emitted but all failed, pass `docs` as-is
        // (empty) and let `enrich` decide: it can still extract from the stub
        // alone or return Err, which the loop counts as a failure.
        let enrichment_docs = if plans.is_empty() {
            std::slice::from_ref(&doc)
        } else {
            &docs
        };
        match adapter.enrich(stub, enrichment_docs, source) {
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
    let jobs = client.policy().global_concurrency;
    let robots = Arc::new(RobotsCache::new());
    let mut sorted: Vec<&SourceSpec> = sources.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut pending = sorted.into_iter();
    let mut tasks = tokio::task::JoinSet::new();
    let mut task_sources = HashMap::with_capacity(jobs.min(sources.len()));
    let mut results = Vec::with_capacity(sources.len());

    loop {
        while tasks.len() < jobs {
            let Some(source) = pending.next() else {
                break;
            };
            // Factory failures also belong to the source, not the whole scan.
            let adapter = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                adapter_factory(source)
            })) {
                Ok(adapter) => adapter,
                Err(_) => {
                    results.push(failed_source(&source.id));
                    continue;
                }
            };
            let client = client.clone();
            let robots = robots.clone();
            let spec = source.clone();
            let task = tasks.spawn(async move {
                fetch_source(&client, &spec, adapter.as_ref(), &robots, deadline).await
            });
            task_sources.insert(task.id(), source.id.as_str());
        }
        let Some(result) = tasks.join_next_with_id().await else {
            break;
        };
        match result {
            Ok((id, result)) => {
                task_sources.remove(&id);
                results.push(result);
            }
            Err(error) => {
                if let Some(source) = task_sources.remove(&error.id()) {
                    results.push(failed_source(source));
                }
            }
        }
    }
    results.sort_by(|a, b| a.health.source.cmp(&b.health.source));
    results
}

fn failed_source(id: &str) -> SourceFetchResult {
    SourceFetchResult {
        candidates: Vec::new(),
        health: SourceHealth {
            source: id.to_owned(),
            status: SourceStatus::ParseError,
            duration_ms: 0,
            requests: 0,
            events: 0,
        },
    }
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
