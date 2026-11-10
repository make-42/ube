use crate::{sources::{self, Source}, types::{Kind, SearchResult}};
use moka::future::Cache;
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

pub type CacheKey = (Kind, String, u32, &'static str); // kind, normalized query, page, source

pub struct Config {
    pub bind: String,
    pub cache_ttl: Duration,
    pub source_timeout: Duration,
    pub max_pages: u32,
    pub cache_capacity: u64,
    pub page_max_age: u64,
    /// Runtime weight overrides from `SOURCE_WEIGHTS="duckduckgo=2,dockerhub=0.3"`.
    pub weights: HashMap<String, f64>,
}

fn env<T: FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            bind: env("BIND", "0.0.0.0:3000".to_string()),
            cache_ttl: Duration::from_secs(env("CACHE_TTL_SECS", 300)),
            source_timeout: Duration::from_millis(env("SOURCE_TIMEOUT_MS", 8000)),
            max_pages: env("MAX_PAGES", 10),
            cache_capacity: env("CACHE_MAX_ENTRIES", 10_000),
            page_max_age: env("PAGE_MAX_AGE_SECS", 86_400),
            weights: parse_weights(&env("SOURCE_WEIGHTS", String::new())),
        }
    }
}

/// Parse `"name=weight,name=weight"`. Entries that are malformed, negative or
/// not finite are skipped with a warning.
pub fn parse_weights(s: &str) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('=').map(|(k, v)| (k.trim(), v.trim().parse::<f64>())) {
            Some((k, Ok(w))) if !k.is_empty() && w.is_finite() && w >= 0.0 => {
                out.insert(k.to_string(), w);
            }
            _ => tracing::warn!("SOURCE_WEIGHTS: ignoring {part:?} (expected name=non-negative-number)"),
        }
    }
    out
}

pub struct Page {
    pub html: &'static str,
    pub etag: String,
    pub cache_control: String,
}

pub struct AppState {
    pub cfg: Config,
    pub sources: HashMap<Kind, Vec<Source>>,
    pub cache: Cache<CacheKey, Arc<Vec<SearchResult>>>,
    pub page: Page,
}

const TEMPLATE: &str = include_str!("../static/search.html");

impl AppState {
    pub fn new(cfg: Config) -> Self {
        let html = TEMPLATE
            .replace("__CACHE_TTL_MS__", &cfg.cache_ttl.as_millis().to_string())
            .replace("__SOURCE_TIMEOUT_MS__", &cfg.source_timeout.as_millis().to_string());
        let mut h = DefaultHasher::new();
        html.hash(&mut h);
        let page = Page {
            etag: format!("\"{:x}\"", h.finish()),
            cache_control: format!(
                "public, max-age={}, stale-while-revalidate=604800",
                cfg.page_max_age
            ),
            html: Box::leak(html.into_boxed_str()),
        };
        let cache = Cache::builder()
            .max_capacity(cfg.cache_capacity)
            .time_to_live(cfg.cache_ttl)
            .build();
        Self { sources: sources::registry(), cache, page, cfg }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_weights;

    #[test]
    fn parses_weights_and_skips_bad_entries() {
        let w = parse_weights(" duckduckgo = 2 ,dockerhub=0.3, bad, x=-1, y=nan, =3, z=abc ,");
        assert_eq!(w.len(), 2);
        assert_eq!(w["duckduckgo"], 2.0);
        assert_eq!(w["dockerhub"], 0.3);
        assert!(parse_weights("").is_empty());
    }
}
