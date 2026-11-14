//! MediaWiki API engine, ported from SearXNG's mediawiki.py.
//!
//! A configurable engine for MediaWiki instances that provide a JSON API at
//! `/w/api.php`. This allows 12 MediaWiki instances to be ported with one
//! codebase (e.g., Wikinews, Wiktionary, Wikibooks, Gentoo Wiki, NixOS Wiki,
//! Minecraft Wiki, Free Software Directory, and others).
//!
//! The MediaWiki search API returns results with HTML snippets that contain
//! `<span class="searchmatch">` for the matched terms. The `html_to_text`
//! utility from `foundations::util` can be used to extract plain text.
//!
//! URL format: `<base>/wiki/<Title with _>`, plus `#<sectiontitle>` when present.
//! Snippets may contain HTML; convert to text using `html_to_text`.
//!
//! # Configuration
//!
//! Register in `pages.rs` (or `news.rs` for Wikinews) with:
//! ```
//! Source::new("wikinews", mediawiki::mediawiki_with(cfg))
//! ```
//!
//! # References
//!
//! - SearXNG: `searx/engines/mediawiki.py`
//! - MediaWiki API: https://www.mediawiki.org/wiki/API:Main_page
//! - Search API: https://www.mediawiki.org/wiki/API:Main_page#Searching

use crate::sources::foundations::util::html_to_text;
use crate::types::SearchResult;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

const DEFAULT_PAGE_SIZE: u32 = 5;
const DEFAULT_FIRST_PAGE: u32 = 1;

#[derive(Debug, Clone)]
pub struct MediaWikiConfig {
    pub base_url: String,
    pub search_type: String,
    pub page_size: u32,
    pub first_page: u32,
    pub extra_fields: HashMap<String, String>,
    pub headers: HashMap<String, String>,
}

impl Default for MediaWikiConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            search_type: "text".into(),
            page_size: DEFAULT_PAGE_SIZE,
            first_page: DEFAULT_FIRST_PAGE,
            extra_fields: HashMap::new(),
            headers: HashMap::new(),
        }
    }
}

impl MediaWikiConfig {
    pub fn new(url: &str) -> Self {
        Self {
            base_url: url.to_string(),
            search_type: "text".into(),
            page_size: DEFAULT_PAGE_SIZE,
            first_page: DEFAULT_FIRST_PAGE,
            extra_fields: HashMap::new(),
            headers: HashMap::new(),
        }
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    pub fn with_search_type(mut self, t: &str) -> Self {
        self.search_type = t.to_string();
        self
    }

    pub fn with_page_size(mut self, size: u32) -> Self {
        self.page_size = size;
        self
    }

    pub fn with_first_page(mut self, page: u32) -> Self {
        self.first_page = page;
        self
    }

    pub fn with_extra_field(mut self, key: &str, value: &str) -> Self {
        self.extra_fields.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("ube/0.1")
            .build()
            .expect("failed to build MediaWiki HTTP client")
    })
}

pub fn build_url(cfg: &MediaWikiConfig, q: &str, page: u32) -> Url {
    let base = Url::parse(&cfg.base_url).expect("invalid base URL");
    let mut url = base.join("/w/api.php").expect("failed to build URL");
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("list", "search")
        .append_pair("format", "json")
        .append_pair("srsearch", &q.replace(" ", "+"))
        .append_pair("srlimit", &cfg.page_size.to_string())
        .append_pair("sroffset", &((page - 1) * cfg.page_size).to_string())
        .append_pair("srwhat", &cfg.search_type);
    url
}

pub fn parse(json: &serde_json::Value, page: u32, cfg: &MediaWikiConfig) -> Vec<SearchResult> {
    let mut out = Vec::new();
    let query = get_path(json, "query.search");
    let results = query.as_array().map(|arr| arr.as_slice()).unwrap_or(&[]);
    let rank_offset = ((page.max(1) - 1) * cfg.page_size) as usize;
    
    for (idx, item) in results.iter().enumerate() {
        let item = item.as_object().expect("result item is not an object");
        let title = get_str(item, "title");
        if title.is_empty() { continue; }
        
        let display_title = title.replace('_', " ");
        let base = cfg.base_url.trim_end_matches('/');
        let url = format!("{}/wiki/{}", base, title);
        
        let snippet = get_str(item, "snippet");
        let mut description = if snippet.is_empty() {
            String::new()
        } else {
            html_to_text(&snippet)
        };
        
        let section = get_str(item, "section");
        if !section.is_empty() {
            description.push(' ');
            description.push_str(&section);
        }
        
        let rank = rank_offset + idx;
        let score = 1.0 / (1.0 + rank as f64);
        let mut result = SearchResult::new(display_title, url.as_str(), description, score);
        
        for (key, field) in &cfg.extra_fields {
            let value = get_str(item, field);
            if !value.is_empty() {
                result = result.with(key, value);
            }
        }
        
        out.push(result);
    }
    
    out
}

fn get_str(obj: &serde_json::Map<String, serde_json::Value>, path: &str) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current_value: Option<&serde_json::Value> = Some(&serde_json::Value::Object(obj.clone()));
    for part in parts {
        match current_value {
            Some(v) if v.is_object() => {
                current_value = v.as_object().and_then(|o| o.get(part));
            }
            _ => return String::new(),
        }
    }
    current_value.and_then(serde_json::Value::as_str).map(|s| s.to_string()).unwrap_or(String::new())
}

fn get_path<'a>(obj: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = obj;
    for part in parts {
        if let serde_json::Value::Object(o) = current {
            current = o.get(part).unwrap_or(&serde_json::Value::Null);
        } else {
            break;
        }
    }
    current
}

pub async fn mediawiki(q: String, page: u32) -> Result<Vec<SearchResult>> {
    let cfg = MediaWikiConfig {
        base_url: "https://en.wikipedia.org".into(),
        search_type: "text".into(),
        page_size: DEFAULT_PAGE_SIZE,
        first_page: DEFAULT_FIRST_PAGE,
        extra_fields: HashMap::new(),
        headers: HashMap::new(),
    };
    mediawiki_with(cfg, q, page).await
}

pub async fn mediawiki_with(
    cfg: MediaWikiConfig,
    q: String,
    page: u32,
) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let url = build_url(&cfg, &q, page);
    
    let mut headers = cfg.headers.clone();
    headers.insert("User-Agent".to_string(), "ube/0.1".to_string());
    
    let mut req = client().get(url);
    for (k, v) in &headers {
        req = req.header(k.as_str(), v.as_str());
    }
    
    let resp = req.send().await.context("MediaWiki request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let excerpt: String = resp.text().await.context("failed to read response body")?;
        bail!("MediaWiki returned HTTP {}: {}", status, excerpt.chars().take(300).collect::<String>());
    }
    
    let body = resp.text().await.context("failed to read response body")?;
    let json: serde_json::Value = serde_json::from_str(&body).context("MediaWiki returned invalid JSON")?;
    let results = parse(&json, page, &cfg);
    Ok(results)
}

pub fn wikipedia_config() -> MediaWikiConfig {
    MediaWikiConfig::new("https://en.wikipedia.org").with_search_type("text").with_page_size(5)
}

pub fn wikinews_config() -> MediaWikiConfig {
    MediaWikiConfig::new("https://en.wikinews.org").with_search_type("text").with_page_size(5)
}

pub fn wiktionary_config() -> MediaWikiConfig {
    MediaWikiConfig::new("https://en.wiktionary.org").with_search_type("text").with_page_size(5)
}

pub fn wikibooks_config() -> MediaWikiConfig {
    MediaWikiConfig::new("https://en.wikibooks.org").with_search_type("text").with_page_size(5)
}

pub fn gentoo_config() -> MediaWikiConfig {
    MediaWikiConfig::new("https://wiki.gentoo.org").with_search_type("text").with_page_size(5)
}

pub fn nixos_config() -> MediaWikiConfig {
    MediaWikiConfig::new("https://wiki.nixos.org").with_search_type("text").with_page_size(5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_cfg() -> MediaWikiConfig {
        MediaWikiConfig {
            base_url: "https://en.wikipedia.org".into(),
            search_type: "text".into(),
            page_size: 5,
            first_page: 1,
            extra_fields: HashMap::new(),
            headers: HashMap::new(),
        }
    }

    #[test]
    fn builds_url_correctly() {
        let cfg = make_cfg();
        let url = build_url(&cfg, "test query", 1);
        let url_str = url.to_string();
        assert!(url_str.contains("action=query"));
        assert!(url_str.contains("list=search"));
        assert!(url_str.contains("srsearch=test%2Bquery"));
        assert!(url_str.contains("srlimit=5"));
        assert!(url_str.contains("sroffset=0"));
        assert!(url_str.contains("srwhat=text"));
    }

    #[test]
    fn parses_basic_response() {
        let json = json!({
            "query": {
                "search": [
                    {
                        "title": "Rust_(programming_language)",
                        "pageid": "66566",
                        "snippet": "<span class=\"searchmatch\">Rust</span> is a systems programming language",
                        "section": ""
                    }
                ]
            }
        });
        let r = parse(&json, 1, &make_cfg());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Rust (programming language)");
        assert_eq!(r[0].url, "https://en.wikipedia.org/wiki/Rust_(programming_language)");
        assert!((r[0].score - 1.0).abs() < 1e-9);
        assert!(r[0].description.contains("Rust"));
    }

    #[test]
    fn converts_underscores_to_spaces() {
        let json = json!({
            "query": {
                "search": [
                    {
                        "title": "Gentoo_Linux",
                        "pageid": "12345",
                        "snippet": "Gentoo is an operating system",
                        "section": ""
                    }
                ]
            }
        });
        let r = parse(&json, 1, &make_cfg());
        assert_eq!(r[0].title, "Gentoo Linux");
    }

    #[test]
    fn extracts_html_snippet_text() {
        let json = json!({
            "query": {
                "search": [
                    {
                        "title": "Test",
                        "pageid": "12345",
                        "snippet": "<span class=\"searchmatch\">Hello   world</span><br><span>More text</span>",
                        "section": ""
                    }
                ]
            }
        });
        let r = parse(&json, 1, &make_cfg());
        assert!(r[0].description.contains("Hello"));
        assert!(r[0].description.contains("world"));
    }

    #[test]
    fn includes_section_title() {
        let json = json!({
            "query": {
                "search": [
                    {
                        "title": "Test",
                        "pageid": "12345",
                        "snippet": "Some content",
                        "section": "Section 2"
                    }
                ]
            }
        });
        let cfg = make_cfg();
        let r = parse(&json, 1, &cfg);
        assert!(r[0].description.contains("Section 2"));
    }

    #[test]
    fn skips_missing_title() {
        let json = json!({
            "query": {
                "search": [
                    {
                    }
                ]
            }
        });
        let r = parse(&json, 1, &make_cfg());
        assert!(r.is_empty());
    }

    #[test]
    fn scores_continue_across_pages() {
        let json = json!({
            "query": {
                "search": [
                    { "title": "A", "pageid": "1", "snippet": "A", "section": "" },
                    { "title": "B", "pageid": "2", "snippet": "B", "section": "" }
                ]
            }
        });
        let r = parse(&json, 2, &make_cfg());
        assert!((r[0].score - 1.0 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn handles_empty_results() {
        let json = json!({ "query": { "search": [] } });
        let r = parse(&json, 1, &make_cfg());
        assert!(r.is_empty());
    }

    #[test]
    fn handles_missing_query_key() {
        let json = json!({});
        let r = parse(&json, 1, &make_cfg());
        assert!(r.is_empty());
    }
}
