//! Generic HTML search engine, ported from SearXNG's `html_engine.py`.
//!
//! A configurable engine for HTML search endpoints that use CSS selectors.
//! It follows SearXNG's `html_engine` pattern: CSS selectors for result
//! containers, links, titles, content, thumbnails, and suggestions.
//!
//! XPath does not translate 1:1 to CSS: `concat()`/`substring-after()` need
//! a post-processing closure. This engine uses `scraper` crate for CSS
//! selectors and URL resolution.
//!
//! Result `extra` keys are configured by the user via `html_engine.rs` config:
//! `extra_fields` maps extra keys to selector paths.
//!
//! Example usage: Register in `pages.rs` with a config like:
//! ```
//! Source::new("lobste-rs", html_engine::html_engine_with(cfg))
//! ```

use crate::types::SearchResult;
use scraper::{ElementRef, Html, Selector};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

use anyhow::{bail, Context, Result};

/// SearXNG: "to simplify the page count lets use the default of 10 results per page".
const DEFAULT_PAGE_SIZE: u32 = 10;
const DEFAULT_FIRST_PAGE: u32 = 1;

static DEFAULT_HEADERS: std::sync::LazyLock<HashMap<String, String>> = std::sync::LazyLock::new(|| HashMap::from_iter([
    ("accept".to_string(), "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_string()),
    ("accept-language".to_string(), "en-US,en;q=0.9".to_string()),
    ("sec-ch-ua".to_string(), "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\"".to_string()),
    ("sec-ch-ua-mobile".to_string(), "?0".to_string()),
    ("sec-ch-ua-platform".to_string(), "\"Windows\"".to_string()),
    ("upgrade-insecure-requests".to_string(), "1".to_string()),
]));

/// CSS selectors for result items.
pub struct Selectors {
    /// Selector for result container elements (e.g., `<li>` wrapping a result).
    pub item: Selector,
    /// Selector for the link element within the item (e.g., `<a>` tag).
    pub link: Selector,
    /// Selector for the title element (e.g., `<h2>` or `<a>` text).
    pub title: Selector,
    /// Selector for content/snippet elements.
    pub content: Selector,
    /// Optional selector for thumbnail images.
    pub thumbnail: Option<Selector>,
}

/// Configuration for the generic HTML engine.
///
/// This struct allows configuring any HTML-based search engine by specifying
/// the URL endpoint, CSS selectors, and field mappings.
///
/// # Fields
///
/// * `base_url` - The base URL of the search endpoint.
/// * `search_url` - The search endpoint path (relative to `base_url`).
/// * `page_size` - Number of results per page (default: 10).
/// * `first_page` - The first page number (default: 1).
/// * `item_selector` - CSS selector for result container elements.
/// * `link_selector` - CSS selector for result links within items.
/// * `title_selector` - CSS selector for result titles.
/// * `content_selector` - CSS selector for result snippets/content.
/// * `thumbnail_selector` - Optional CSS selector for thumbnail images.
/// * `extra_fields` - Map of extra keys to selector paths (e.g., `"duration": ".duration"`).
/// * `headers` - Additional HTTP headers to send with requests.
#[derive(Debug, Clone)]
pub struct HtmlEngineConfig {
    pub base_url: String,
    pub search_url: String,
    pub page_size: u32,
    pub first_page: u32,
    pub item_selector: String,
    pub link_selector: String,
    pub title_selector: String,
    pub content_selector: String,
    pub thumbnail_selector: Option<String>,
    pub extra_fields: HashMap<String, String>,
    pub headers: HashMap<String, String>,
}

impl Default for HtmlEngineConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            search_url: String::new(),
            page_size: DEFAULT_PAGE_SIZE,
            first_page: DEFAULT_FIRST_PAGE,
            item_selector: String::new(),
            link_selector: String::new(),
            title_selector: String::new(),
            content_selector: String::new(),
            thumbnail_selector: None,
            extra_fields: HashMap::new(),
            headers: DEFAULT_HEADERS.clone(),
        }
    }
}

impl HtmlEngineConfig {
    /// Create a new config with the given base URL and search URL.
    pub fn new(base_url: &str, search_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            search_url: search_url.to_string(),
            ..Default::default()
        }
    }

    /// Set the page size.
    pub fn with_page_size(mut self, size: u32) -> Self {
        self.page_size = size;
        self
    }

    /// Set the first page number.
    pub fn with_first_page(mut self, page: u32) -> Self {
        self.first_page = page;
        self
    }

    /// Set the item selector.
    pub fn with_item_selector(mut self, selector: &str) -> Self {
        self.item_selector = selector.to_string();
        self
    }

    /// Set the link selector.
    pub fn with_link_selector(mut self, selector: &str) -> Self {
        self.link_selector = selector.to_string();
        self
    }

    /// Set the title selector.
    pub fn with_title_selector(mut self, selector: &str) -> Self {
        self.title_selector = selector.to_string();
        self
    }

    /// Set the content selector.
    pub fn with_content_selector(mut self, selector: &str) -> Self {
        self.content_selector = selector.to_string();
        self
    }

    /// Set the thumbnail selector (optional).
    pub fn with_thumbnail_selector(mut self, selector: Option<String>) -> Self {
        self.thumbnail_selector = selector;
        self
    }

    /// Set an extra field selector.
    pub fn with_extra_field(mut self, key: &str, selector: &str) -> Self {
        self.extra_fields.insert(key.to_string(), selector.to_string());
        self
    }

    /// Set additional HTTP headers.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }
}

/// Build the request URL from config.
///
/// # Examples
///
/// ```
/// use ube::sources::engines::html_engine::HtmlEngineConfig;
///
/// let cfg = HtmlEngineConfig::new("https://example.org", "/search");
/// let url = html_engine::build_url(&cfg, "test query", 1);
/// assert_eq!(url.as_str(), "https://example.org/search?q=test+query&page=1");
/// ```
pub fn build_url(cfg: &HtmlEngineConfig, q: &str, page: u32) -> Url {
    let mut url = Url::parse(&cfg.base_url).expect("invalid base_url");
    let search_path = if url.path().ends_with('/') { format!("{}{}", url.path().trim_end_matches('/'), cfg.search_url) } else { format!("{}{}", url.path(), cfg.search_url) };
    url.set_path(search_path.as_str());
    url.query_pairs_mut()
        .append_pair("q", q)
        .append_pair("page", &page.to_string());
    url
}

/// Parse HTML response into search results.
///
/// This is a pure function with no network I/O. It uses CSS selectors to
/// extract results from the HTML body.
///
/// # Arguments
///
/// * `body` - The HTML response body.
/// * `base_url` - The base URL for resolving relative links.
/// * `page` - The page number (used for scoring).
///
/// # Returns
///
/// A vector of `SearchResult` structs, or an empty vector if no results found.
pub fn parse(body: &str, base_url: &Url, page: u32, cfg: &HtmlEngineConfig) -> Vec<SearchResult> {
    let s = selectors();

    // Strip noscript tags if present (Bing-style wrapping)
    let body = crate::sources::foundations::util::strip_noscript(body);

    let dom = Html::parse_document(&body);
    let mut out = Vec::new();

    for item in dom.select(&s.item) {
        // Find the link within the item
        let Some(link) = item.select(&s.link).next() else { continue };
        let Some(href) = link.value().attr("href") else { continue };

        // Resolve relative URLs
        let Ok(url) = base_url.join(href) else { continue };

        // Get title from link or item
        let title = if let Some(title_el) = item.select(&s.title).next() {
            text(title_el)
        } else {
            text(link)
        };

        if title.is_empty() {
            continue;
        }

        // Get content snippet
        let content = if let Some(content_el) = item.select(&s.content).next() {
            text(content_el)
        } else {
            String::new()
        };

        // Get thumbnail if selector is provided
        let icon = if let Some(thumb_sel) = &s.thumbnail {
            if let Some(thumb) = item.select(thumb_sel).next() {
                if let Some(src) = thumb.value().attr("src") {
                    if let Ok(icon_url) = base_url.join(src) {
                        Some(icon_url.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Calculate rank-based score
        let rank = (page.max(1) - 1) as usize * cfg.page_size as usize + out.len();
        let score = 1.0 / (1.0 + rank as f64);

        let mut result = SearchResult::new(title, url.as_str(), content, score);

        if let Some(icon_url) = icon {
            result = result.with("icon", icon_url);
        }

        for (key, selector) in &cfg.extra_fields {
            let selector = Selector::parse(selector).expect("invalid selector");
            if let Some(extra_el) = item.select(&selector).next() {
                if let Some(extra_text) = text_opt(extra_el) {
                    result = result.with(key, extra_text);
                }
            }
        }

        out.push(result);
    }

    out
}

/// Get selectors from config.
fn selectors() -> &'static Selectors {
    static S: OnceLock<Selectors> = OnceLock::new();
    S.get_or_init(|| {
        let p = |s: &str| Selector::parse(s).expect("valid selector");
        Selectors {
            item: p(&DEFAULT_HTML_ENGINE_CONFIG.item_selector),
            link: p(&DEFAULT_HTML_ENGINE_CONFIG.link_selector),
            title: p(&DEFAULT_HTML_ENGINE_CONFIG.title_selector),
            content: p(&DEFAULT_HTML_ENGINE_CONFIG.content_selector),
            thumbnail: DEFAULT_HTML_ENGINE_CONFIG
                .thumbnail_selector
                .as_ref()
                .map(|s| p(s.as_str())),
        }
    })
}

static DEFAULT_HTML_ENGINE_CONFIG: std::sync::LazyLock<HtmlEngineConfig> = std::sync::LazyLock::new(|| HtmlEngineConfig {
    base_url: "https://example.org".to_string(),
    search_url: "/search".to_string(),
    page_size: DEFAULT_PAGE_SIZE,
    first_page: DEFAULT_FIRST_PAGE,
    item_selector: "div.result".to_string(),
    link_selector: "a[href]".to_string(),
    title_selector: "h3 a".to_string(),
    content_selector: "p".to_string(),
    thumbnail_selector: Some("img.thumbnail".to_string()),
    extra_fields: HashMap::new(),
    headers: DEFAULT_HEADERS.clone(),
});

/// Get the text content of an element, stripping whitespace.
fn text(el: ElementRef<'_>) -> String {
    el.text()
        .flat_map(|s| s.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Get optional text content of an element.
fn text_opt(el: ElementRef<'_>) -> Option<String> {
    let t = text(el);
    if t.is_empty() { None } else { Some(t) }
}

/// Client builder (placeholder for future use).
fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
            .build()
            .expect("failed to build HTML engine client")
    })
}

/// Default entry point: `Source::new("lobste-rs", html_engine::html_engine)`.
pub async fn html_engine(q: String, page: u32) -> Result<Vec<SearchResult>> {
    html_engine_with(DEFAULT_HTML_ENGINE_CONFIG.clone(), q, page).await
}

/// Search with custom configuration.
pub async fn html_engine_with(
    cfg: HtmlEngineConfig,
    q: String,
    page: u32,
) -> Result<Vec<SearchResult>> {
    let url = build_url(&cfg, &q, page);
    let mut builder = client().get(url);
    for (key, value) in cfg.headers.iter() {
        builder = builder.header(key.as_str(), value.as_str());
    }
    let resp = builder.send()
        .await
        .context("HTML engine request failed")?;
    let status = resp.status();
    let body = resp.text().await.context("failed to read HTML engine response")?;

    if !status.is_success() {
        bail!("HTML engine returned HTTP {status}");
    }

    let base = Url::parse(&cfg.base_url).context("invalid base_url")?;
    Ok(parse(&body, &base, page, &cfg))
}

/// Example config for lobste.rs (from SearXNG's lobste-rs.py).
pub fn lobste_rs_config() -> HtmlEngineConfig {
    HtmlEngineConfig::new("https://lobste.rs", "/search")
        .with_page_size(15)
        .with_item_selector("li.item")
        .with_link_selector("a.link")
        .with_title_selector(".title a")
        .with_content_selector(".comment")
        .with_extra_field("score", ".score")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_url() {
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let url = build_url(&cfg, "test query", 1);
        assert_eq!(url.as_str(), "https://example.org/search?q=test+query&page=1");
    }

    #[test]
    fn test_build_url_page_2() {
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let url = build_url(&cfg, "test query", 2);
        assert_eq!(url.as_str(), "https://example.org/search?q=test+query&page=2");
    }

    #[test]
    fn test_parse_empty_html() {
        let base = Url::parse("https://example.org").unwrap();
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let results = parse("<html><body></body></html>", &base, 1, &cfg);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_parse_with_noscript() {
        let base = Url::parse("https://example.org").unwrap();
        let html = r#"<noscript>
            <div class="result">
                <a href="/item/1">Result 1</a>
                <p>Snippet 1</p>
            </div>
        </noscript>"#;
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let results = parse(html, &base, 1, &cfg);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title.as_str(), "Result 1");
    }

    #[test]
    fn test_parse_multiple_results() {
        let base = Url::parse("https://example.org").unwrap();
        let html = r#"
            <div class="result">
                <a href="/item/1">First</a>
                <p>First snippet</p>
            </div>
            <div class="result">
                <a href="/item/2">Second</a>
                <p>Second snippet</p>
            </div>
        "#;
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let results = parse(html, &base, 1, &cfg);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title.as_str(), "First");
        assert_eq!(results[1].title.as_str(), "Second");
    }

    #[test]
    fn test_parse_skips_missing_links() {
        let base = Url::parse("https://example.org").unwrap();
        let html = r#"
            <div class="result">
                <a href="/item/1">First</a>
            </div>
            <div class="result">
                <p>No link here</p>
            </div>
        "#;
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let results = parse(html, &base, 1, &cfg);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parse_skips_empty_titles() {
        let base = Url::parse("https://example.org").unwrap();
        let html = r#"
            <div class="result">
                <a href="/item/1">First</a>
            </div>
            <div class="result">
                <a href="/item/2">   </a>
                <p>Empty title</p>
            </div>
        "#;
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let results = parse(html, &base, 1, &cfg);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parse_scores() {
        let base = Url::parse("https://example.org").unwrap();
        let html = r#"
            <div class="result">
                <a href="/item/1">First</a>
            </div>
            <div class="result">
                <a href="/item/2">Second</a>
            </div>
        "#;
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let results = parse(html, &base, 1, &cfg);
        assert!((results[0].score - 1.0).abs() < 1e-9);
        assert!((results[1].score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_parse_page_2_scores() {
        let base = Url::parse("https://example.org").unwrap();
        let html = r#"
            <div class="result">
                <a href="/item/1">First</a>
            </div>
        "#;
        let cfg = HtmlEngineConfig::new("https://example.org", "/search");
        let results = parse(html, &base, 2, &cfg);
        assert!((results[0].score - 0.09090909090909091).abs() < 1e-9); // rank 10, score = 1/11
    }

    #[test]
    fn test_extra_fields() {
        let base = Url::parse("https://example.org").unwrap();
        let cfg = HtmlEngineConfig::new("https://example.org", "/search")
            .with_extra_field("score", ".score");
        let html = r#"
            <div class="result">
                <a href="/item/1">First</a>
                <span class="score">100 points</span>
            </div>
        "#;
        let results = parse(html, &base, 1, &cfg);
        assert_eq!(results[0].extra.get("score").unwrap(), "100 points");
    }

    #[test]
    fn test_lobste_rs_config() {
        let cfg = lobste_rs_config();
        assert_eq!(cfg.base_url, "https://lobste.rs");
        assert_eq!(cfg.page_size, 15);
        assert_eq!(cfg.item_selector, "li.item");
        assert_eq!(cfg.link_selector, "a.link");
        assert_eq!(cfg.title_selector, ".title a");
    }

    #[test]
    fn test_text_extraction() {
        let html = Html::parse_document("<div>  Hello   World  </div>");
        let el = html.root_element();
        let text = text(el);
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_text_opt_empty() {
        let html = Html::parse_document("<div>   </div>");
        let el = html.root_element();
        assert!(text_opt(el).is_none());
    }
}
