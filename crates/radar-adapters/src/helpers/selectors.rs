//! Static and bounded per-thread runtime selector caches.
use scraper::Selector;
use std::cell::RefCell;
use std::sync::OnceLock;

const MAX_RUNTIME_SELECTORS: usize = 256;

pub(crate) fn cached_selector(selector_str: &'static str) -> Option<&'static Selector> {
    static SELECTORS: OnceLock<std::collections::HashMap<&'static str, Selector>> = OnceLock::new();
    let map = SELECTORS.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        let candidates: &[&'static str] = &[
            "p",
            "time",
            r#"[class*="date"], [id*="date"], [class*="time"], [id*="time"], [class*="when"], [id*="when"]"#,
            r#"[class*="location"], [id*="location"], [class*="venue"], [id*="venue"], [class*="place"], [id*="place"], [class*="address"], [id*="address"]"#,
            "a",
            "iframe",
            "video",
            "audio",
            "meta",
            "h1",
            "title",
            r#"script[type="application/ld+json"]"#,
        ];
        for &s in candidates {
            if let Ok(sel) = Selector::parse(s) {
                m.insert(s, sel);
            }
        }
        m
    });
    map.get(selector_str)
}

thread_local! {
    static RUNTIME_SELECTOR_CACHE: RefCell<std::collections::HashMap<String, Selector>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Parse `selector_str` with a per-thread cache (stored in
/// `RUNTIME_SELECTOR_CACHE`). On a cache hit, returns a clone of the previously
/// parsed [`Selector`]; on a miss, parses, caches, and returns the selector.
/// Avoids repeated CSS parsing on every `enrich` call (HCM: ~1000 events ×
/// 2–5 selectors per source).
pub(crate) fn cached_selector_runtime(selector_str: &str) -> Result<Selector, String> {
    RUNTIME_SELECTOR_CACHE.with(|cache| {
        if let Some(sel) = cache.borrow().get(selector_str).cloned() {
            return Ok(sel);
        }
        let sel = Selector::parse(selector_str).map_err(|e| e.to_string())?;
        let mut cache = cache.borrow_mut();
        if cache.len() < MAX_RUNTIME_SELECTORS {
            cache.insert(selector_str.to_string(), sel.clone());
        }
        Ok(sel)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_cache_is_bounded_without_rejecting_uncached_selectors() {
        RUNTIME_SELECTOR_CACHE.with(|cache| cache.borrow_mut().clear());
        for i in 0..MAX_RUNTIME_SELECTORS * 2 {
            assert!(cached_selector_runtime(&format!(".item-{i}")).is_ok());
        }
        RUNTIME_SELECTOR_CACHE
            .with(|cache| assert_eq!(cache.borrow().len(), MAX_RUNTIME_SELECTORS));
        assert!(cached_selector_runtime("]").is_err());
    }
}
