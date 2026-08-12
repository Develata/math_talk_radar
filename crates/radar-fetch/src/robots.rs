//! Robots policy (§16). `respect_robots = true` is the only mode; no bypass.
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RobotsPolicy {
    #[default]
    Respect,
}

impl RobotsPolicy {
    pub fn is_respected(self) -> bool {
        matches!(self, RobotsPolicy::Respect)
    }
}

// ---- RFC 9309 parser ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RobotsRule {
    Allow(String),
    Disallow(String),
}

#[derive(Debug, Clone, Default)]
pub struct RobotsRules {
    pub rules: Vec<RobotsRule>,
}

impl RobotsRules {
    /// Conservative rules: disallow every path. Used when robots.txt is
    /// unavailable (5xx) or redirects to an unsafe scheme, per RFC 9309 §2.3.1.3.
    pub fn disallow_all() -> Self {
        Self {
            rules: vec![RobotsRule::Disallow("/".to_string())],
        }
    }

    pub fn is_allowed(&self, url: &Url) -> bool {
        let path = url.path();
        let mut best_match: Option<(usize, bool)> = None; // (length, is_allow)
        for rule in &self.rules {
            let (pattern, is_allow) = match rule {
                RobotsRule::Allow(p) => (p.as_str(), true),
                RobotsRule::Disallow(p) => (p.as_str(), false),
            };
            if pattern.is_empty() {
                continue;
            }
            if path.starts_with(pattern) {
                let len = pattern.len();
                match best_match {
                    None => best_match = Some((len, is_allow)),
                    Some((best_len, _)) if len > best_len => best_match = Some((len, is_allow)),
                    Some((best_len, _)) if len == best_len && is_allow => {
                        best_match = Some((len, true));
                    }
                    _ => {}
                }
            }
        }
        match best_match {
            None => true,
            Some((_, is_allow)) => is_allow,
        }
    }
}

/// Hand-rolled RFC 9309 parser. Only reads `User-agent: *` blocks.
pub fn parse_robots(txt: &str) -> RobotsRules {
    let mut rules = Vec::new();
    let mut group_has_wildcard = false;
    let mut in_group = false;

    for line in txt.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f.trim().to_ascii_lowercase(), v.trim()),
            None => continue,
        };
        match field.as_str() {
            "user-agent" => {
                if !in_group {
                    group_has_wildcard = false;
                    in_group = true;
                }
                if value == "*" {
                    group_has_wildcard = true;
                }
            }
            "allow" => {
                in_group = false;
                if group_has_wildcard && !value.is_empty() {
                    rules.push(RobotsRule::Allow(value.to_string()));
                }
            }
            "disallow" => {
                in_group = false;
                if group_has_wildcard {
                    rules.push(RobotsRule::Disallow(value.to_string()));
                }
            }
            _ => {
                in_group = false;
            }
        }
    }
    RobotsRules { rules }
}

// ---- Cache (Oracle #2: thundering-herd fix) ----

pub struct RobotsCache {
    map: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::OnceCell<RobotsRules>>>>,
}

impl RobotsCache {
    pub fn new() -> Self {
        Self {
            map: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_or_init<F, Fut>(
        &self,
        url: &Url,
        fetch: F,
    ) -> Result<RobotsRules, crate::error::FetchError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<RobotsRules, crate::error::FetchError>>,
    {
        let key = host_key(url);
        let cell = {
            let mut map = self.map.lock().await;
            map.entry(key)
                .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
                .clone()
        };
        let rules = cell.get_or_try_init(fetch).await?;
        Ok(rules.clone())
    }
}

impl Default for RobotsCache {
    fn default() -> Self {
        Self::new()
    }
}

fn host_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or("");
    let scheme = url.scheme();
    match url.port() {
        Some(p) => format!("{scheme}://{host}:{p}"),
        None => format!("{scheme}://{host}"),
    }
}

#[cfg(test)]
mod robots_tests {
    use super::*;

    #[test]
    fn parse_and_check() {
        let txt = "User-agent: *\nDisallow: /private\nAllow: /public\n";
        let rules = parse_robots(txt);
        let u1 = Url::parse("https://example.com/private/secret").unwrap();
        assert!(!rules.is_allowed(&u1));
        let u2 = Url::parse("https://example.com/public/page").unwrap();
        assert!(rules.is_allowed(&u2));
        let u3 = Url::parse("https://example.com/other").unwrap();
        assert!(rules.is_allowed(&u3));
    }

    #[test]
    fn empty_robots_allows_all() {
        let rules = parse_robots("");
        let u = Url::parse("https://example.com/anything").unwrap();
        assert!(rules.is_allowed(&u));
    }
}
