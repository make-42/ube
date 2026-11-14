//! Generic JSON engine, ported from SearXNG's `json_engine.py`.
//!
//! A configurable engine for pure-JSON search endpoints. It follows SearXNG's
//! `json_engine` pattern: dot notation paths for request parameters and
//! response fields. This allows 8 SearXNG instances to be ported with one
//! codebase (e.g., MDN, Packagist, Mankier, Wiby, Encylosearch, Crowdview, Openaire).

use crate::types::SearchResult;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct JsonEngineConfig {
    pub search_url: String,
    pub results_path: String,
    pub url_path: String,
    pub title_path: String,
    pub content_path: String,
    pub thumbnail_path: Option<String>,
    pub page_size: u32,
    pub first_page: u32,
    pub headers: HashMap<String, String>,
}

impl Default for JsonEngineConfig {
    fn default() -> Self {
        Self {
            search_url: String::new(),
            results_path: "results".into(),
            url_path: "url".into(),
            title_path: "title".into(),
            content_path: "content".into(),
            thumbnail_path: None,
            page_size: 10,
            first_page: 1,
            headers: [("User-Agent".into(), "ube/0.1".into())].into(),
        }
    }
}

impl JsonEngineConfig {
    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }
    pub fn with_search_url(mut self, url: &str) -> Self {
        self.search_url = url.to_string();
        self
    }
    pub fn with_results_path(mut self, path: &str) -> Self {
        self.results_path = path.to_string();
        self
    }
    pub fn with_url_path(mut self, path: &str) -> Self {
        self.url_path = path.to_string();
        self
    }
    pub fn with_title_path(mut self, path: &str) -> Self {
        self.title_path = path.to_string();
        self
    }
    pub fn with_content_path(mut self, path: &str) -> Self {
        self.content_path = path.to_string();
        self
    }
    pub fn with_thumbnail_path(mut self, path: &str) -> Self {
        self.thumbnail_path = Some(path.to_string());
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
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("ube/0.1")
            .build()
            .expect("failed to build JSON engine HTTP client")
    })
}

pub fn build_url(cfg: &JsonEngineConfig, q: &str, page: u32) -> String {
    let mut url = format!("{}?", cfg.search_url.trim_end_matches('/'));
    let mut params = Vec::new();
    params.push(format!("q={}", q));
    params.push(format!("page={}", page));
    url.push_str(&params.join("&"));
    url
}

pub fn parse(json: &Value, page: u32, cfg: &JsonEngineConfig) -> Vec<SearchResult> {
    let mut out = Vec::new();
    let results = get_path(json, &cfg.results_path);
    let results = results.as_array().map(|arr| arr.as_slice()).unwrap_or(&[]);

    let rank_offset = ((page.max(1) - 1) * cfg.page_size) as usize;

    for (idx, item) in results.iter().enumerate() {
        let empty_map: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        let item = item.as_object().unwrap_or(&empty_map);
        let url = get_str(item, &cfg.url_path);
        if url.is_empty() { continue; }
        let title = get_str(item, &cfg.title_path);
        if title.is_empty() { continue; }
        let description = get_str(item, &cfg.content_path);

        let rank = rank_offset + idx;
        let score = 1.0 / (1.0 + rank as f64);
        let mut result = SearchResult::new(title, url, description, score);

        if let Some(thumb_path) = &cfg.thumbnail_path {
            let thumb = get_str(item, thumb_path);
            if !thumb.is_empty() {
                result = result.with("thumbnail", thumb);
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

fn get_path<'a>(obj: &'a Value, path: &str) -> &'a Value {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = obj;

    for part in parts {
        if let Value::Object(o) = current {
            current = o.get(part).unwrap_or(&Value::Null);
        } else {
            break;
        }
    }

    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_cfg() -> JsonEngineConfig {
        JsonEngineConfig {
            search_url: "https://example.com/search".into(),
            results_path: "results".into(),
            url_path: "url".into(),
            title_path: "title".into(),
            content_path: "description".into(),
            thumbnail_path: None,
            page_size: 10,
            first_page: 1,
            headers: HashMap::new(),
        }
    }

    #[test]
    fn parses_basic_response() {
        let json = json!({
            "results": [
                { "title": "First", "url": "https://example.com/1", "description": "First result" },
                { "title": "Second", "url": "https://example.com/2", "description": "Second result" }
            ]
        });
        let r = parse(&json, 1, &make_cfg());
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "First");
        assert_eq!(r[0].url, "https://example.com/1");
        assert_eq!(r[0].description, "First result");
        assert!((r[0].score - 1.0).abs() < 1e-9);
        assert!((r[1].score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn skips_missing_url() {
        let json = json!({
            "results": [
                { "title": "Good", "url": "https://example.com/good", "description": "Good" },
                { "title": "No URL", "description": "Bad" }
            ]
        });
        let r = parse(&json, 1, &make_cfg());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Good");
    }

    #[test]
    fn skips_missing_title() {
        let json = json!({
            "results": [
                { "url": "https://example.com/good", "description": "Good" },
                { "title": "Good", "url": "https://example.com/good2", "description": "Good2" }
            ]
        });
        let r = parse(&json, 1, &make_cfg());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Good");
    }

    #[test]
    fn scores_continue_across_pages() {
        let json = json!({
            "results": [
                { "title": "A", "url": "https://a.com", "description": "A" },
                { "title": "B", "url": "https://b.com", "description": "B" }
            ]
        });
        let r = parse(&json, 2, &make_cfg());
        assert!((r[0].score - 1.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn handles_nested_paths() {
        let cfg = JsonEngineConfig {
            search_url: "https://example.com/search".into(),
            results_path: "data.results".into(),
            url_path: "href".into(),
            title_path: "title".into(),
            content_path: "description".into(),
            thumbnail_path: None,
            page_size: 10,
            first_page: 1,
            headers: HashMap::new(),
        };
        let json = json!({
            "data": {
                "results": [
                    { "title": "Nested", "href": "https://example.com/nested", "description": "Nested result" }
                ]
            }
        });
        let r = parse(&json, 1, &cfg);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Nested");
        assert_eq!(r[0].url, "https://example.com/nested");
    }

    #[test]
    fn handles_optional_thumbnail() {
        let cfg = JsonEngineConfig {
            search_url: "https://example.com/search".into(),
            results_path: "results".into(),
            url_path: "url".into(),
            title_path: "title".into(),
            content_path: "description".into(),
            thumbnail_path: Some("thumbnail".into()),
            page_size: 10,
            first_page: 1,
            headers: HashMap::new(),
        };
        let json = json!({
            "results": [
                { "title": "With Thumb", "url": "https://example.com/with", "description": "Has thumb", "thumbnail": "https://thumb.com" }
            ]
        });
        let r = parse(&json, 1, &cfg);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "With Thumb");
        assert_eq!(r[0].extra.get("thumbnail").and_then(|v| v.as_str()), Some("https://thumb.com"));
    }

    #[test]
    fn handles_empty_results() {
        let json = json!({ "results": [] });
        let r = parse(&json, 1, &make_cfg());
        assert!(r.is_empty());
    }

    #[test]
    fn handles_missing_results_key() {
        let json = json!({});
        let r = parse(&json, 1, &make_cfg());
        assert!(r.is_empty());
    }
}
