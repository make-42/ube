//! MDN Web Docs search, ported from SearXNG's `mdn.py` (uses `json_engine`).
//!
//! Paging:   1-based `page`, 10 results per page.
//! Notes:    MDN URLs are relative (e.g., `/en-US/docs/...`); they are prefixed
//!           with `https://developer.mozilla.org`. Results carry `popularity`
//!           which is mapped to an `extra` key "popularity".

use crate::types::SearchResult;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};

const BASE_URL: &str = "https://developer.mozilla.org";
const PAGE_SIZE: u32 = 10;

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("ube/0.1")
            .build()
            .expect("failed to build MDN HTTP client")
    })
}

/// Default entry point: `Source::new("mdn", mdn::mdn_pages)`.
pub async fn mdn_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    mdn_pages_with(&Config::new(), q, page).await
}

pub async fn mdn_pages_with(cfg: &Config, q: String, page: u32) -> Result<Vec<SearchResult>> {
    let page = page.max(1);

    let resp = client()
        .get(format!("{BASE_URL}/api/v1/search"))
        .query(&[("q", q.as_str()), ("page", &page.to_string())])
        .header("accept", "application/json")
        .send()
        .await
        .context("MDN request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read MDN response body")?;

    if status.as_u16() == 429 {
        bail!("MDN rate limited the request (HTTP 429)");
    }
    if !status.is_success() {
        let excerpt: String = body.chars().take(300).collect();
        bail!("MDN returned HTTP {status}: {excerpt:?}");
    }

    let json: Value = serde_json::from_str(&body).context("MDN returned invalid JSON")?;
    let results = parse(&json, page, cfg);

    if results.is_empty() {
        let title = body.split_once("<title>").and_then(|(_, r)| r.split_once("</title>")).map_or("", |(t, _)| t);
        tracing::warn!(
            "mdn: 0 results for {q:?} page {page}: HTTP {status}, {} bytes, title {title:?}, start {:?}",
            body.len(),
            body.chars().take(120).collect::<String>()
        );
    }

    Ok(results)
}

/// Config for MDN instance settings.
#[derive(Debug, Clone)]
pub struct Config {
    pub base_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: BASE_URL.to_string(),
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Build the query URL.
pub fn build_url(cfg: &Config, q: &str, page: u32) -> String {
    let mut url = format!("{}/api/v1/search?", cfg.base_url.trim_end_matches('/'));
    url.push_str(&format!("q={}", q));
    url.push_str(&format!("&page={}", page));
    url
}

/// Pure parse function.
pub fn parse(json: &Value, page: u32, _cfg: &Config) -> Vec<SearchResult> {
    let mut out = Vec::new();
    let documents = json.get("documents")
        .and_then(|v| v.as_array())
        .map(|arr| arr.as_slice()).unwrap_or(&[]);

    let rank_offset = ((page.max(1) - 1) * PAGE_SIZE) as usize;

    for (idx, doc) in documents.iter().enumerate() {
        let empty_map: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        let doc = doc.as_object().unwrap_or(&empty_map);

        // MDN returns relative URLs; prefix them
        let relative_url = doc.get("mdn_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_start_matches('/'))
            .unwrap_or("");

        if relative_url.is_empty() {
            continue;
        }

        let url = format!("{}/{}", BASE_URL, relative_url);

        let title = doc.get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let description = doc.get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let rank = rank_offset + idx;
        let score = 1.0 / (1.0 + rank as f64);

        let mut result = SearchResult::new(title, url.as_str(), description, score);

        // Map popularity to extra
        if let Some(pop) = doc.get("popularity").and_then(|v| v.as_f64()) {
            result = result.with("popularity", format!("{:.6}", pop));
        }

        out.push(result);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "documents": [
                {
                    "mdn_url": "/en-US/docs/WebAssembly/Guides/Rust_to_Wasm",
                    "title": "Compiling from Rust to WebAssembly",
                    "summary": "This tutorial shows you how to compile a Rust project into WebAssembly.",
                    "popularity": 0.010722850781209029
                },
                {
                    "mdn_url": "/en-US/docs/Web/JavaScript/Guide/Language_overview",
                    "title": "Language Overview",
                    "summary": "Overview of JavaScript language features.",
                    "popularity": 0.005
                }
            ]
        })
    }

    fn make_cfg() -> Config {
        Config::new()
    }

    #[test]
    fn parses_basic_response() {
        let r = parse(&fixture(), 1, &make_cfg());
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Compiling from Rust to WebAssembly");
        assert!(r[0].url.starts_with("https://developer.mozilla.org"));
        assert_eq!(
            r[0].url,
            "https://developer.mozilla.org/en-US/docs/WebAssembly/Guides/Rust_to_Wasm"
        );
        assert_eq!(r[0].description, "This tutorial shows you how to compile a Rust project into WebAssembly.");
        assert!((r[0].score - 1.0).abs() < 1e-9);
        assert!((r[1].score - 0.5).abs() < 1e-9);
        assert_eq!(
            r[0].extra.get("popularity").and_then(|v| v.as_str()),
            Some("0.010723")
        );
    }

    #[test]
    fn scores_continue_across_pages() {
        let r = parse(&fixture(), 2, &make_cfg());
        assert!((r[0].score - 1.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn skips_missing_mdn_url() {
        let json = json!({
            "documents": [
                { "mdn_url": "/docs/a", "title": "A", "summary": "A" },
                { "mdn_url": "", "title": "No URL", "summary": "B" },
                { "title": "No URL Key", "summary": "C" }
            ]
        });
        let r = parse(&json, 1, &make_cfg());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].url, "https://developer.mozilla.org/docs/a");
    }

    #[test]
    fn handles_empty_results() {
        let json = json!({ "documents": [] });
        assert!(parse(&json, 1, &make_cfg()).is_empty());
    }

    #[test]
    fn handles_missing_documents_key() {
        let json = json!({});
        assert!(parse(&json, 1, &make_cfg()).is_empty());
    }
}
