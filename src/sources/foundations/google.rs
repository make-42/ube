//! Google-specific search helpers (F3 foundation).
//!
//! This module provides shared functionality for Google's tab-specific searches
//! (images, news, videos, scholar, maps, etc.). The key feature is the `tbm`
//! parameter handling, which tells Google which type of results to return.
//!
//! See [`GoogleConfig::with_tbm`] for how to set the tab type.
//! See [`build_google_url`] for constructing search URLs with optional `tbm`.
//!
//! # Tab types
//!
//! - `"isch"`: Google Images
//! - `"nws"`: Google News
//! - `"vid"`: Google Videos
//! - `"sch"`: Google Scholar
//! - `"maps"`: Google Maps
//!
//! # Usage
//!
//! ```ignore
//! let cfg = GoogleConfig::default();
//! let cfg = cfg.with_tbm("isch");
//! let url = build_google_url(&cfg, "query", 1, None).unwrap();
//! ```

use url::Url;

/// Build a Google search URL with optional tab type parameter (`tbm`).
///
/// The `tbm` parameter tells Google which tab to search in:
/// - `"isch"`: Images
/// - `"nws"`: News
/// - `"vid"`: Videos
/// - `"sch"`: Scholar
/// - `"maps"`: Maps
///
/// If `tbm` is `None`, the standard web search is used.
///
/// # Arguments
///
/// - `cfg`: Google configuration (includes `hl`, `lr`, `cr`, etc.)
/// - `q`: Search query
/// - `page`: Page number (1-based)
/// - `tbm`: Optional tab type
///
/// # Returns
///
/// A `Result<String>` containing the constructed search URL.
pub fn build_google_url(
    cfg: &GoogleConfig,
    q: &str,
    page: u32,
    tbm: Option<&str>,
) -> Result<String, url::ParseError> {
    // Build query parameters as a vector of key-value pairs
    let mut params: Vec<(String, String)> = Vec::new();
    params.push(("q".to_string(), q.to_string()));
    
    if let Some(cr) = &cfg.cr {
        params.push(("cr".to_string(), cr.to_string()));
    }
    params.push(("hl".to_string(), cfg.hl.clone()));
    params.push(("lr".to_string(), cfg.lr.clone()));
    
    if let Some(t) = cfg.time_range {
        params.push(("tbs".to_string(), format!("qdr:{}", t)));
    }
    
    if let Some(s) = cfg.safe {
        params.push(("safe".to_string(), s.to_string()));
    }
    
    if page > 1 {
        params.push(("start".to_string(), ((page - 1) * 10).to_string()));
    }
    
    if let Some(tbm) = tbm {
        params.push(("tbm".to_string(), tbm.to_string()));
    }
    
    for (k, v) in &cfg.extra {
        params.push((k.clone(), v.clone()));
    }
    
    // Build URL with properly encoded query string
    // Use simple encoding: keys and values are already strings, just join them
    let query_string = params.into_iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k.as_str()), urlencoding::encode(v.as_str())))
        .collect::<Vec<_>>()
        .join("&");
    Ok(format!("https://www.google.com/search?{}", query_string))
}

/// Google configuration with optional tab type support.
///
/// This extends the standard `GoogleConfig` to support different Google search tabs.
/// Use `with_tbm()` to set the tab type (e.g., "isch" for images, "nws" for news).
#[derive(Clone, Debug)]
pub struct GoogleConfig {
    /// Interface language, e.g. "en", "fr", "zh-TW".
    pub hl: String,

    /// Restrict results to a language, e.g. "lang_en".
    /// Empty means any language.
    pub lr: String,

    /// Restrict results to a country, e.g. "countryFR".
    pub cr: Option<String>,

    /// "off" | "medium" | "high".
    pub safe: Option<&'static str>,

    /// "d" | "w" | "m" | "y" (day/week/month/year).
    pub time_range: Option<&'static str>,

    /// Extra query arguments.
    pub extra: Vec<(String, String)>,

    /// Tab type parameter (`tbm`).
    /// None = web search, Some("isch") = images, etc.
    pub tbm: Option<String>,
}

impl Default for GoogleConfig {
    fn default() -> Self {
        Self {
            hl: "en".into(),
            lr: "lang_en".into(),
            cr: None,
            safe: None,
            time_range: None,
            extra: vec![],
            tbm: None,
        }
    }
}

impl GoogleConfig {
    /// Set the tab type for Google search.
    ///
    /// Returns a new config with the specified tab type.
    ///
    /// # Tab types
    ///
    /// - `None`: Web search (default)
    /// - `"isch"`: Google Images
    /// - `"nws"`: Google News
    /// - `"vid"`: Google Videos
    /// - `"sch"`: Google Scholar
    /// - `"maps"`: Google Maps
    ///
    /// # Example
    ///
    /// ```ignore
    /// let cfg = GoogleConfig::default().with_tbm("isch");
    /// ```
    pub fn with_tbm(mut self, tbm: impl Into<String>) -> Self {
        self.tbm = Some(tbm.into());
        self
    }

    /// Build a search URL with this configuration and optional tab type.
    ///
    /// # Arguments
    ///
    /// - `q`: Search query
    /// - `page`: Page number (1-based)
    /// - `tbm`: Optional tab type to override config's tbm
    pub fn build_url(&self, q: &str, page: u32, tbm: Option<&str>) -> Result<String, url::ParseError> {
        build_google_url(self, q, page, tbm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_web_search_url() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test query", 1, None).unwrap();

        assert!(url.contains("https://www.google.com/search"));
        assert!(url.contains("q=test%20query"));
        assert!(url.contains("hl=en"));
        assert!(!url.contains("tbm="));
    }

    #[test]
    fn builds_images_search_url() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test query", 1, Some("isch")).unwrap();

        assert!(url.contains("tbm=isch"));
    }

    #[test]
    fn builds_news_search_url() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test query", 1, Some("nws")).unwrap();

        assert!(url.contains("tbm=nws"));
    }

    #[test]
    fn builds_videos_search_url() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test query", 1, Some("vid")).unwrap();

        assert!(url.contains("tbm=vid"));
    }

    #[test]
    fn builds_scholar_search_url() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test query", 1, Some("sch")).unwrap();

        assert!(url.contains("tbm=sch"));
    }

    #[test]
    fn builds_maps_search_url() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test query", 1, Some("maps")).unwrap();

        assert!(url.contains("tbm=maps"));
    }

    #[test]
    fn with_tbm_returns_new_config() {
        let cfg = GoogleConfig::default();
        let cfg_with_tbm = cfg.with_tbm("isch");

        assert_eq!(cfg_with_tbm.tbm, Some("isch".to_string()));
    }

    #[test]
    fn build_url_uses_config_tbm_when_none() {
        let cfg = GoogleConfig {
            hl: "fr".into(),
            lr: "lang_fr".into(),
            ..Default::default()
        };
        let url = cfg.build_url("test", 1, None).unwrap();

        assert!(url.contains("hl=fr"));
        assert!(url.contains("lr=lang_fr"));
        assert!(!url.contains("tbm="));
    }

    #[test]
    fn build_url_overrides_config_tbm() {
        let cfg = GoogleConfig {
            hl: "en".into(),
            lr: "lang_en".into(),
            tbm: Some("vid".to_string()), // config says videos
            ..Default::default()
        };
        // But override with "isch"
        let url = cfg.build_url("test", 1, Some("isch")).unwrap();

        assert!(url.contains("tbm=isch"));
    }

    #[test]
    fn page_parameter_uses_1_based() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test", 3, None).unwrap();

        assert!(url.contains("start=20"));
    }

    #[test]
    fn page_one_has_no_start_param() {
        let cfg = GoogleConfig::default();
        let url = build_google_url(&cfg, "test", 1, None).unwrap();

        assert!(!url.contains("start="));
    }

    #[test]
    fn time_range_parameter_is_added() {
        let cfg = GoogleConfig {
            hl: "en".into(),
            lr: "lang_en".into(),
            time_range: Some("d"),
            ..Default::default()
        };
        let url = build_google_url(&cfg, "test", 1, None).unwrap();

        assert!(url.contains("tbs=") && url.contains("qdr%3Ad"));
    }

    #[test]
    fn safe_search_parameter_is_added() {
        let cfg = GoogleConfig {
            hl: "en".into(),
            lr: "lang_en".into(),
            safe: Some("active"),
            ..Default::default()
        };
        let url = build_google_url(&cfg, "test", 1, None).unwrap();

        assert!(url.contains("safe=active"));
    }

    #[test]
    fn extra_parameters_are_added() {
        let cfg = GoogleConfig {
            hl: "en".into(),
            lr: "lang_en".into(),
            extra: vec![("custom".to_string(), "foo".to_string())],
            ..Default::default()
        };
        let url = build_google_url(&cfg, "test", 1, None).unwrap();

        assert!(url.contains("custom") && url.contains("foo"));
    }

    #[test]
    fn country_parameter_is_added() {
        let cfg = GoogleConfig {
            hl: "en".into(),
            lr: "lang_en".into(),
            cr: Some("countryUS".to_string()),
            ..Default::default()
        };
        let url = build_google_url(&cfg, "test", 1, None).unwrap();

        assert!(url.contains("cr=countryUS"));
    }
}
