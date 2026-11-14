//! Wikipedia engine, ported from SearXNG's `wikipedia.py`.
//!
//! Unlike traditional search engines, Wikipedia does not support searching
//! across all pages. Instead, this engine performs a **single-page lookup**
//! by title. It uses the REST `/api/rest_v1/page/summary/<Title>` endpoint
//! which returns ONE page (or 404/400 if not found).
//!
//! URL format: `https://{wiki_netloc}/api/rest_v1/page/summary/{title}`
//!
//! # Configuration
//!
//! Register in `pages.rs` with:
//! ```
//! Source::new("wikipedia", wikipedia::wikipedia_pages)
//! ```

use crate::types::SearchResult;
use anyhow::{bail, Context, Result};
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

const DEFAULT_PAGE_SIZE: u32 = 1;

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0")
            .build()
            .expect("failed to build Wikipedia HTTP client")
    })
}

pub fn build_url(wiki_netloc: &str, _title: &str) -> String {
    format!("{}/api/rest_v1/page/summary/{}", wiki_netloc, _title)
}

pub async fn wikipedia_pages(q: String, _page: u32) -> Result<Vec<SearchResult>> {
    let page = _page.max(1);
    let query = q;

    let title = urlencoding::encode(&query);
    let url = format!("https://en.wikipedia.org/api/rest_v1/page/summary/{}", title);

    let resp = client()
        .get(&url)
        .header("accept-language", "en-US,en;q=0.9")
        .send()
        .await
        .context("Wikipedia request failed")?;

    let status = resp.status();
    if status.as_u16() == 404 {
        tracing::warn!("wikipedia: 404 for {query:?} - title may not exist");
        return Ok(Vec::new());
    }
    if status.as_u16() == 400 {
        let body = resp.text().await.context("failed to read response body")?;
        tracing::warn!("wikipedia: 400 for {query:?}: {body:?}");
        return Ok(Vec::new());
    }
    if !status.is_success() {
        let excerpt: String = resp.text().await.context("failed to read response body")?;
        bail!("Wikipedia returned HTTP {}: {}", status, excerpt.chars().take(300).collect::<String>());
    }

    let body = resp.text().await.context("failed to read response body")?;
    let json: serde_json::Value = serde_json::from_str(&body).context("Wikipedia returned invalid JSON")?;

    let results = parse(&json, page);
    if results.is_empty() {
        tracing::warn!("wikipedia: 0 results for {query:?} page {page}: HTTP {status}, {} bytes", body.len());
    }
    Ok(results)
}

fn parse(json: &serde_json::Value, page: u32) -> Vec<SearchResult> {
    let mut out = Vec::new();

    // Title is plain text from the API
    let title = json.get("title")
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.get("titles")
                .and_then(|v| v.as_object())
                .and_then(|o| o.get("display"))
                .and_then(|v| v.as_str())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    if title.is_empty() {
        return out;
    }

    let wikipedia_link = json.get("content_urls")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("desktop"))
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("page"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Extract is plain text (not HTML) from the Wikipedia API
    let extract = json.get("extract")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    let img_src = json.get("thumbnail")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("source"))
        .and_then(|v| v.as_str());

    let score = 1.0 / (1.0 + (page.max(1) - 1) as f64 * DEFAULT_PAGE_SIZE as f64);

    let mut result = SearchResult::new(title, wikipedia_link, extract, score)
        .icon("https://wikipedia.org/favicon.ico");

    if let Some(thumb) = img_src {
        result = result.with("thumbnail", thumb);
    }

    out.push(result);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_basic_response() {
        let json = json!({
            "title": "Rust (programming language)",
            "extract": "Rust is a systems programming language based on ownership.",
            "content_urls": {
                "desktop": {
                    "page": "https://en.wikipedia.org/wiki/Rust_(programming_language)"
                }
            },
            "thumbnail": {
                "source": "https://upload.wikimedia.org/wikipedia/commons/thumb/2/2a/Rust-lang-logo.svg/600px-Rust-lang-logo.svg.png"
            }
        });

        let r = parse(&json, 1);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Rust (programming language)");
        assert_eq!(r[0].description, "Rust is a systems programming language based on ownership.");
    }

    #[test]
    fn parses_without_thumbnail() {
        let json = json!({
            "title": "Rust",
            "extract": "Rust is a programming language.",
            "content_urls": {
                "desktop": {
                    "page": "https://en.wikipedia.org/wiki/Rust"
                }
            }
        });

        let r = parse(&json, 1);
        assert_eq!(r.len(), 1);
        assert!(r[0].extra.get("thumbnail").is_none());
    }

    #[test]
    fn empty_title_is_skipped() {
        let json = json!({
            "title": "",
            "extract": "Some content",
            "content_urls": {
                "desktop": {
                    "page": "https://en.wikipedia.org/wiki/Test"
                }
            }
        });

        let r = parse(&json, 1);
        assert!(r.is_empty());
    }

    #[test]
    fn scores_are_1_0_for_first_page() {
        let json = json!({
            "title": "Test",
            "extract": "Test content",
            "content_urls": {
                "desktop": {
                    "page": "https://en.wikipedia.org/wiki/Test"
                }
            }
        });

        let r = parse(&json, 1);
        assert!((r[0].score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn preserves_spaces_in_plain_text_extract() {
        let json = json!({
            "title": "Test",
            "extract": "Hey, HEY, or Hey! may refer to:",
            "content_urls": {
                "desktop": {
                    "page": "https://en.wikipedia.org/wiki/Test"
                }
            }
        });

        let r = parse(&json, 1);
        assert_eq!(r[0].description, "Hey, HEY, or Hey! may refer to:");
    }
}
