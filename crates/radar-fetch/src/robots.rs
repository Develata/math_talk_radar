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

    /// R9-B01: RFC 9309 §2.2.2 matching operates on the full request URI path
    /// including the query string. A rule like `Disallow: /*?next=` can only
    /// match when the query is part of the comparison target. The target is
    /// percent-decoded per RFC 9309 §2.2.1 before pattern matching.
    pub fn is_allowed(&self, url: &Url) -> bool {
        let target_raw = target_path_query(url);
        let decoded = percent_decode_target(&target_raw);
        let target = decoded.as_str();
        let mut best_match: Option<(usize, bool)> = None; // (length, is_allow)
        for rule in &self.rules {
            let (pattern, is_allow) = match rule {
                RobotsRule::Allow(p) => (p.as_str(), true),
                RobotsRule::Disallow(p) => (p.as_str(), false),
            };
            if pattern.is_empty() {
                continue;
            }
            if let Some(len) = matches_robots_pattern(pattern, target) {
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

/// R9-B01: Build the RFC 9309 match target from the URL path and query. When
/// the URL has a non-empty query, the target is `path?query`; otherwise just
/// `path`. The fragment is never part of the match target.
fn target_path_query(url: &Url) -> String {
    match url.query() {
        Some(q) if !q.is_empty() => format!("{}?{}", url.path(), q),
        _ => url.path().to_string(),
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

    // FETCH-1: when the pattern ends with `*` (the last char before the
    // stripped `$` anchor), the `*` can absorb any remaining target chars.
    // Without this, `*$` patterns (e.g. `Disallow: /foo*$`) incorrectly
    // reject paths the `*` should match.
    if !pat.is_empty() && pat[pat.len() - 1] == '*' {
        return true;
    }

    if anchored_end { ti == tgt.len() } else { true }
}

/// R9-B02: the crawler product token used for RFC 9309 §2.3.1 crawler-specific
/// group matching. Matches the product name in the User-Agent string
/// (`math_talk_radar/<version>`), so a site can publish a group like
/// `User-agent: math_talk_radar\nDisallow: /` to target this crawler
/// specifically. Sites that do not name this crawler fall back to the
/// `User-agent: *` wildcard group.
pub const CRAWLER_TOKEN: &str = "math_talk_radar";

/// Parse robots.txt using the default crawler token ([`CRAWLER_TOKEN`]).
pub fn parse_robots(txt: &str) -> RobotsRules {
    parse_robots_for(txt, CRAWLER_TOKEN)
}

/// R9-B02: RFC 9309 §2.3.1 group-based parser. A robots.txt is a sequence of
/// groups; each group begins with one or more `User-agent` lines and is
/// followed by `Allow`/`Disallow` lines. A `User-agent` line after any rule
/// starts a new group.
///
/// Group selection (RFC 9309 §2.3.1):
/// 1. Collect all groups whose `User-agent` exactly matches `crawler_token`
///    (case-insensitive). Merge their rules.
/// 2. If no specific group matched, use groups with `User-agent: *`.
/// 3. If neither matched, the result is allow-all (no rules).
///
/// Previously the parser only read `User-agent: *` groups, so a site that
/// published `User-agent: math_talk_radar\nDisallow: /` followed by a
/// wildcard allow-all group would see this crawler ignore its specific rules.
pub fn parse_robots_for(txt: &str, crawler_token: &str) -> RobotsRules {
    #[derive(Default)]
    struct Group {
        agents: Vec<String>,
        rules: Vec<RobotsRule>,
    }

    let mut groups: Vec<Group> = Vec::new();
    let mut current: Option<Group> = None;
    let mut in_agents = true;

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
                if !in_agents {
                    if let Some(g) = current.take() {
                        groups.push(g);
                    }
                    current = Some(Group::default());
                    in_agents = true;
                }
                current
                    .get_or_insert_with(Group::default)
                    .agents
                    .push(value.to_ascii_lowercase());
            }
            "allow" => {
                in_agents = false;
                if let Some(g) = current.as_mut()
                    && !value.is_empty()
                {
                    g.rules.push(RobotsRule::Allow(value.to_string()));
                }
            }
            "disallow" => {
                in_agents = false;
                if let Some(g) = current.as_mut() {
                    g.rules.push(RobotsRule::Disallow(value.to_string()));
                }
            }
            _ => {
                in_agents = false;
            }
        }
    }
    if let Some(g) = current.take() {
        groups.push(g);
    }

    let token_lower = crawler_token.to_ascii_lowercase();
    let selected: Vec<&Group> = groups
        .iter()
        .filter(|g| g.agents.iter().any(|a| a == &token_lower))
        .collect();
    let selected = if selected.is_empty() {
        groups
            .iter()
            .filter(|g| g.agents.iter().any(|a| a == "*"))
            .collect()
    } else {
        selected
    };

    let mut rules = Vec::new();
    for g in selected {
        rules.extend(g.rules.iter().cloned());
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

    /// Get cached rules for `url`'s origin + `allowlist_key`, or initialize
    /// by calling `fetch`. The cache key includes the allowlist so that
    /// sources with different host policies get separate cache entries
    /// (B6-1: a broad-allowlist source's cached robots rules must not be
    /// reused by a narrow-allowlist source for the same origin — the narrow
    /// source's `fetch_robots_txt` would have rejected cross-host redirects
    /// that the broad source accepted).
    pub async fn get_or_init<F, Fut>(
        &self,
        url: &Url,
        allowlist_key: &str,
        fetch: F,
    ) -> Result<RobotsRules, crate::error::FetchError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<RobotsRules, crate::error::FetchError>>,
    {
        let key = format!("{}|{allowlist_key}", host_key(url));
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

/// RFC 9309 §2.2.1: robots path matching operates on the percent-decoded
/// target (path + query). `Url::path()` returns the encoded form, so we decode
/// `%XX` sequences (including multi-byte UTF-8) before matching against rule
/// patterns.
fn percent_decode_target(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = hex_digit(bytes[i + 1]);
            let l = hex_digit(bytes[i + 2]);
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
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

    // FETCH-8: RFC 9309 §2.2.1 requires matching against the percent-decoded
    // path. A disallow rule for /café must block the encoded form /caf%C3%A9.
    #[test]
    fn percent_encoded_path_matches_decoded_rule() {
        let txt = "User-agent: *\nDisallow: /café\n";
        let rules = parse_robots(txt);
        let u = Url::parse("https://example.com/caf%C3%A9").unwrap();
        assert!(
            !rules.is_allowed(&u),
            "percent-encoded path must match decoded rule"
        );
    }

    // R9-B01: a Disallow rule targeting the query must match when the request
    // URL carries that query. `Disallow: /*?next=` blocks /events?next=5 but
    // allows /events (no query).
    #[test]
    fn query_specific_disallow_rule_matches() {
        let txt = "User-agent: *\nDisallow: /*?next=\n";
        let rules = parse_robots(txt);
        let blocked = Url::parse("https://example.com/events?next=5").unwrap();
        assert!(
            !rules.is_allowed(&blocked),
            "Disallow: /*?next= must block URLs with ?next= query"
        );
        let allowed = Url::parse("https://example.com/events").unwrap();
        assert!(
            rules.is_allowed(&allowed),
            "Disallow: /*?next= must not block URLs without a query"
        );
    }

    // R9-B02: a crawler-specific group must be honored. A site publishing
    // `User-agent: math_talk_radar\nDisallow: /` must block this crawler even
    // if a wildcard allow-all group follows.
    #[test]
    fn crawler_specific_group_honored() {
        let txt = "User-agent: math_talk_radar\nDisallow: /\nUser-agent: *\nAllow: /\n";
        let rules = parse_robots(txt);
        let u = Url::parse("https://example.com/any").unwrap();
        assert!(
            !rules.is_allowed(&u),
            "crawler-specific Disallow: / must block this crawler"
        );
    }

    // R9-B02: when no crawler-specific group exists, the wildcard group is used.
    #[test]
    fn wildcard_fallback_when_no_specific_group() {
        let txt = "User-agent: *\nDisallow: /private\n";
        let rules = parse_robots(txt);
        let u = Url::parse("https://example.com/private").unwrap();
        assert!(
            !rules.is_allowed(&u),
            "wildcard group must be used as fallback"
        );
    }

    // R9-B02: when both a specific and wildcard group exist, only the specific
    // group's rules apply (RFC 9309 §2.3.1: the crawler must not merge specific
    // and wildcard groups).
    #[test]
    fn specific_group_takes_precedence_over_wildcard() {
        let txt = "\
User-agent: math_talk_radar
Disallow: /specific

User-agent: *
Disallow: /wildcard
";
        let rules = parse_robots(txt);
        let specific = Url::parse("https://example.com/specific").unwrap();
        let wildcard = Url::parse("https://example.com/wildcard").unwrap();
        assert!(
            !rules.is_allowed(&specific),
            "specific group rule must apply"
        );
        assert!(
            rules.is_allowed(&wildcard),
            "wildcard group rule must NOT apply when a specific group matched"
        );
    }

    // R9-B02: multiple groups matching the same crawler token are merged.
    #[test]
    fn multiple_specific_groups_merged() {
        let txt = "\
User-agent: math_talk_radar
Disallow: /a

User-agent: math_talk_radar
Disallow: /b
";
        let rules = parse_robots(txt);
        assert!(!rules.is_allowed(&Url::parse("https://example.com/a").unwrap()));
        assert!(!rules.is_allowed(&Url::parse("https://example.com/b").unwrap()));
        assert!(rules.is_allowed(&Url::parse("https://example.com/c").unwrap()));
    }

    // R9-B02: case-insensitive product token matching per RFC 9309 §2.3.1.
    #[test]
    fn crawler_token_match_is_case_insensitive() {
        let txt = "User-agent: Math_Talk_Radar\nDisallow: /\n";
        let rules = parse_robots(txt);
        let u = Url::parse("https://example.com/any").unwrap();
        assert!(
            !rules.is_allowed(&u),
            "User-agent matching must be case-insensitive"
        );
    }

    // R9-B02: consecutive User-agent lines share the same group's rules.
    #[test]
    fn consecutive_user_agents_share_group() {
        let txt = "\
User-agent: math_talk_radar
User-agent: other_bot
Disallow: /shared

User-agent: *
Allow: /
";
        let rules = parse_robots(txt);
        let u = Url::parse("https://example.com/shared").unwrap();
        assert!(
            !rules.is_allowed(&u),
            "both User-agent lines in a group must share the Disallow rule"
        );
        // other_bot is not our token; verify via parse_robots_for
        let other_rules = parse_robots_for(txt, "other_bot");
        assert!(!other_rules.is_allowed(&u));
    }
}
