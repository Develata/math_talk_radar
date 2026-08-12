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
            if let Some(len) = matches_robots_pattern(pattern, path) {
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

/// RFC 9309 §2.2.2 pattern match. Returns the match length (pattern length) if
/// `path` matches `pattern`, else `None`. `*` matches any sequence including
/// `/`; `$` at end of pattern anchors the match to the end of the path.
/// Patterns without `*` or `$` use plain prefix match (the pre-wildcard
/// behavior), so existing rules are unaffected.
fn matches_robots_pattern(pattern: &str, path: &str) -> Option<usize> {
    // Fast path: no special characters → prefix match (pre-wildcard behavior).
    if !pattern.contains('*') && !pattern.contains('$') {
        return if path.starts_with(pattern) {
            Some(pattern.len())
        } else {
            None
        };
    }

    let pat: Vec<char> = pattern.chars().collect();
    let tgt: Vec<char> = path.chars().collect();

    // `$` at end of pattern anchors the match to the end of the path.
    let (pat, anchored_end): (&[char], bool) = if pat.last() == Some(&'$') {
        (&pat[..pat.len() - 1], true)
    } else {
        (&pat[..], false)
    };

    if glob_match(pat, &tgt, anchored_end) {
        Some(pattern.len())
    } else {
        None
    }
}

/// Two-pointer wildcard matcher (RFC 9309 §2.2.2). `*` matches any sequence
/// including `/`. When `anchored_end` is true the entire path must be consumed
/// (the `$` end-anchor); otherwise a prefix match is accepted.
fn glob_match(pat: &[char], tgt: &[char], anchored_end: bool) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi: Option<usize> = None;
    let mut star_ti: usize = 0;

    while ti < tgt.len() && pi < pat.len() {
        if pat[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if pat[pi] == tgt[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    // Consume trailing `*`s that the loop did not reach.
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }

    if pi < pat.len() {
        return false;
    }

    if anchored_end { ti == tgt.len() } else { true }
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

    #[test]
    fn wildcard_disallow_matches_subpaths() {
        let txt = "User-agent: *\nDisallow: /private/*\n";
        let rules = parse_robots(txt);
        let u1 = Url::parse("https://example.com/private/secret").unwrap();
        assert!(
            !rules.is_allowed(&u1),
            "wildcard * should match any suffix including /"
        );
        let u2 = Url::parse("https://example.com/private/").unwrap();
        assert!(
            !rules.is_allowed(&u2),
            "wildcard * matches the empty suffix"
        );
        let u3 = Url::parse("https://example.com/private").unwrap();
        assert!(
            rules.is_allowed(&u3),
            "wildcard /private/* must not match bare /private (no trailing slash)"
        );
    }

    #[test]
    fn end_anchor_matches_exact_path_only() {
        let txt = "User-agent: *\nDisallow: /end$\n";
        let rules = parse_robots(txt);
        let u1 = Url::parse("https://example.com/end").unwrap();
        assert!(
            !rules.is_allowed(&u1),
            "$ anchors the match to the end of the path"
        );
        let u2 = Url::parse("https://example.com/end/extra").unwrap();
        assert!(
            rules.is_allowed(&u2),
            "$ must not match paths with a suffix beyond the anchor"
        );
    }

    #[test]
    fn longest_match_wins_allow_over_disallow_wildcard() {
        let txt = "User-agent: *\nDisallow: /private/*\nAllow: /private/public\n";
        let rules = parse_robots(txt);
        let u1 = Url::parse("https://example.com/private/public").unwrap();
        assert!(
            rules.is_allowed(&u1),
            "longer Allow (/private/public) must override shorter Disallow wildcard"
        );
        let u2 = Url::parse("https://example.com/private/secret").unwrap();
        assert!(
            !rules.is_allowed(&u2),
            "Disallow wildcard still matches non-allow-listed subpaths"
        );
    }

    // FS-5: an oversized robots.txt (body exceeding max_response_body) must
    // yield disallow-all, not allow-all. The conservative rule set blocks every
    // path so we never crawl without having seen the full policy.
    #[test]
    fn disallow_all_blocks_every_path() {
        let rules = RobotsRules::disallow_all();
        assert!(!rules.is_allowed(&Url::parse("https://example.com/").unwrap()));
        assert!(!rules.is_allowed(&Url::parse("https://example.com/any/path").unwrap()));
        assert!(!rules.is_allowed(&Url::parse("https://example.com/robots.txt").unwrap()));
    }
}
