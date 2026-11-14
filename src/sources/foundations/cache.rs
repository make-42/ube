//! Shared token cache helper for engines that need stateful pagination.
//!
//! Some engines (DuckDuckGo, Vimeo, Pinterest, etc.) require tokens that are
//! bound to a specific query and page number. These tokens are cached per
//! `(engine, key, page)` with a TTL to avoid redundant requests.
//!
//! The cache key is constructed from engine-specific fields plus a query string.
//! The TTL is configurable per engine.

use moka::future::Cache;
use std::sync::OnceLock;
use std::time::Duration;
use std::sync::Arc;

use futures_util::FutureExt;
/// A cached token entry.
#[derive(Clone, Debug)]
pub struct CachedToken {
    /// The cached token value.
    pub token: String,
    /// Timestamp when the token was fetched.
    pub at: std::time::Instant,
}

/// Builder for creating a token cache with configurable TTL and capacity.
///
/// # Example
///
/// ```ignore
/// use crate::sources::foundations::cache::TokenCacheBuilder;
///
/// static C: OnceLock<TokenCacheBuilder> = OnceLock::new();
/// let cache = C.get_or_init(TokenCacheBuilder::new);
/// ```
pub struct TokenCacheBuilder {
    capacity: u64,
    ttl: Duration,
    max_idle: Duration,
}

impl TokenCacheBuilder {
    /// Create a new builder with default values.
    ///
    /// Defaults:
    /// - capacity: 2000
    /// - ttl: 1 hour
    /// - max_idle: 24 hours
    pub fn new() -> Self {
        Self {
            capacity: 2_000,
            ttl: Duration::from_secs(3_600), // 1 hour
            max_idle: Duration::from_secs(86_400), // 24 hours
        }
    }

    /// Set the maximum number of entries in the cache.
    pub fn capacity(mut self, capacity: u64) -> Self {
        self.capacity = capacity;
        self
    }

    /// Set the time-to-live for cached entries.
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Set the maximum idle time for cached entries before removal.
    ///
    /// Defaults to 24 hours if not set.
    pub fn max_idle(mut self, max_idle: Duration) -> Self {
        self.max_idle = max_idle;
        self
    }

    /// Build the cache.
    pub fn build(self) -> TokenCache {
        TokenCache {
            inner: OnceLock::new(),
            capacity: self.capacity,
            ttl: self.ttl,
            max_idle: self.max_idle,
        }
    }
}

/// A shared token cache.
///
/// The cache is keyed by a string (engine-specific key) and stores `CachedToken` values.
pub struct TokenCache {
    inner: OnceLock<Cache<String, CachedToken>>,
    capacity: u64,
    ttl: Duration,
    max_idle: Duration,
}

impl TokenCache {
    /// Get a reference to the inner cache, initializing if needed.
    fn inner(&self) -> &Cache<String, CachedToken> {
        self.inner.get_or_init(|| {
            Cache::builder()
                .max_capacity(self.capacity)
                .time_to_live(self.ttl)
                .build()
        })
    }

    /// Get a cached token or return `None` if not found/expired.
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key (engine-specific)
    /// * `page` - The page number (1-based)
    ///
    /// # Returns
    ///
    /// The cached token if found and not expired, `None` otherwise.
    pub fn get(&self, key: &str, page: u32) -> Option<String> {
        let cache = self.inner();
        let full_key = format!("{}|{}", key, page);
        // moka future cache get returns a Future
        cache.get(&full_key).now_or_never().flatten().map(|t| t.token.clone())
    }

    /// Store a token in the cache.
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key (engine-specific)
    /// * `token` - The token to cache
    /// * `page` - The page number (1-based)
    pub fn set(&self, key: &str, token: String, page: u32) {
        let cache = self.inner();
        let full_key = format!("{}|{}", key, page);
        // moka future cache insert returns a Future
        let _ = cache.insert(full_key, CachedToken { token, at: std::time::Instant::now() }).now_or_never();
    }

    /// Check if a cache entry exists (may be expired).
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key (engine-specific)
    /// * `page` - The page number (1-based)
    ///
    /// # Returns
    ///
    /// `true` if the key exists in the cache (may be expired).
    pub fn contains(&self, key: &str, page: u32) -> bool {
        let cache = self.inner();
        let full_key = format!("{}|{}", key, page);
        // contains_key returns a plain bool, not a Future
        cache.contains_key(&full_key)
    }

    /// Remove all entries for a specific key prefix.
    ///
    /// This is used to clear tokens when they need to be refreshed.
    pub fn clear(&self, key: &str) {
        let cache = self.inner();
        let prefix = format!("{}|", key);
        // Iterate over keys and remove matching entries
        // iter returns a plain iterator, not a Future
        for k in cache.iter() {
            if k.0.starts_with(&prefix) {
                let _ = cache.remove(Arc::as_ref(&k.0));
            }
        }
    }
}

/// Create a token cache with default settings.
///
/// This is a convenience function that creates a cache with:
/// - capacity: 2000
/// - ttl: 1 hour
/// - max_idle: 24 hours
pub fn default_cache() -> TokenCache {
    TokenCacheBuilder::new().build()
}

/// Create a token cache with custom settings.
///
/// # Example
///
/// ```ignore
/// use crate::sources::foundations::cache::TokenCacheBuilder;
///
/// static CACHE: OnceLock<TokenCacheBuilder> = OnceLock::new();
/// let cache = CACHE.get_or_init(TokenCacheBuilder::new);
/// cache.init();
/// ```
pub fn with_config(ttl: Duration, capacity: u64) -> TokenCache {
    TokenCacheBuilder::new()
        .ttl(ttl)
        .capacity(capacity)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_returns_none_for_new_key() {
        let cache = default_cache();
        assert!(cache.get("test_key", 1).is_none());
    }

    #[test]
    fn test_set_and_get() {
        let cache = default_cache();
        cache.set("test_key", "test_token".to_string(), 1);
        assert_eq!(cache.get("test_key", 1), Some("test_token".to_string()));
    }

    #[test]
    fn test_page_separation() {
        let cache = default_cache();
        cache.set("test_key", "page1_token".to_string(), 1);
        cache.set("test_key", "page2_token".to_string(), 2);
        assert_eq!(cache.get("test_key", 1), Some("page1_token".to_string()));
        assert_eq!(cache.get("test_key", 2), Some("page2_token".to_string()));
    }

    #[test]
    fn test_contains() {
        let cache = default_cache();
        assert!(!cache.contains("nonexistent", 1));
        cache.set("test_key", "token".to_string(), 1);
        assert!(cache.contains("test_key", 1));
    }
}
