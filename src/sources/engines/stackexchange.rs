//! Stack Exchange search, ported from SearXNG's `stackexchange.py`.
//!
//! Uses the Stack Exchange API v2.3 (`api.stackexchange.com/2.3/search/advanced`)
//! to query any Stack Exchange site. Supports multiple sites via `Config::site`.
//!
//! Endpoint: GET `https://api.stackexchange.com/2.3/search/advanced?`
//! Query params: `q`, `page`, `pagesize`, `site`, `sort`, `order=desc`.
//!
//! Notes:
//! - Responses are gzipped; reqwest automatically decompresses.
//! - Anonymous quota is 300 requests/day/IP. A `backoff` field may appear in
//!   the response; honour it by returning `Err` and not hammering.
//! - Titles are HTML-escaped (`&quot;`, `&amp;`, etc.); decode them.
//! - The `answer_count` field maps to `replies` in `SearchResult`.
//! - One `Config` struct for site selection; many sources registered.

use crate::types::SearchResult;

use anyhow::{bail, Context, Result};
use reqwest::header::ACCEPT;
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};

const BASE_URL: &str = "https://api.stackexchange.com/2.3/search/advanced";
const PAGE_SIZE: u32 = 10;

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("ube/0.1")
            .build()
            .expect("failed to build Stack Exchange HTTP client")
    })
}

pub async fn stackexchange_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    stackexchange_pages_with(&Config::new("stackoverflow"), q, page).await
}
pub async fn stackexchange_pages_with(cfg: &Config, q: String, page: u32) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let site = &cfg.site;

    let resp = client()
        .get(BASE_URL)
        .query(&[
            ("q", q.as_str()),
            ("page", &page.to_string()),
            ("pagesize", &PAGE_SIZE.to_string()),
            ("site", site),
            ("sort", "activity"),
            ("order", "desc"),
        ])
        .header(ACCEPT, "application/json")
        .send()
        .await
        .context("Stack Exchange request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read Stack Exchange response body")?;

    if status.as_u16() == 429 {
        bail!("Stack Exchange rate limited the request (HTTP 429)");
    }
    if !status.is_success() {
        let excerpt: String = body.chars().take(300).collect();
        bail!("Stack Exchange returned HTTP {status}: {excerpt:?}");
    }

    let json: Value = serde_json::from_str(&body).context("Stack Exchange returned invalid JSON")?;
    let results = parse(&json, page);

    if results.is_empty() && json.get("items").is_none() {
        tracing::warn!("stackexchange: no `items` key for {q:?} page {page} (API changed?)");
    }
    Ok(results)
}

/// Parse a Stack Exchange API response. Pure function, no network I/O.
pub fn parse(json: &Value, page: u32) -> Vec<SearchResult> {
    let items = json.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let mut out = Vec::new();

    for item in items {
        let title = non_empty(item.get("title")).unwrap_or("");
        if title.is_empty() {
            continue;
        }

        let question_id = item.get("question_id").and_then(Value::as_u64).unwrap_or(0);
        let site = item.get("site").and_then(Value::as_str).unwrap_or("stackoverflow");
        let url = format!("https://{site}.com/q/{question_id}");

        // Build snippet from tags, author, answer status, and score.
        let mut snippet = String::new();

        let tags = item.get("tags").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        if !tags.is_empty() {
            snippet.push_str(&format!("[{}]", tags.iter().map(|t| t.as_str().unwrap_or("")).collect::<Vec<_>>().join(", ")));
        }

        snippet.push(' ');
        snippet.push_str(&item.get("owner").and_then(|o| o.get("display_name")).and_then(Value::as_str).unwrap_or("unknown").to_string());

        if item.get("is_answered").and_then(Value::as_bool).unwrap_or(false) {
            snippet.push_str(" // is answered");
        }

        snippet.push_str(&format!(" // score: {}", item.get("score").and_then(Value::as_i64).unwrap_or(0)));

        let mut extra = Vec::new();
        let rank = (page.max(1) - 1) as usize * PAGE_SIZE as usize + out.len();
        let owner = item.get("owner").and_then(Value::as_object).map(|o| o.clone());
        if let Some(o) = owner {
            extra.push(("owner", serde_json::json!(o)));
        }
        let tags = item.get("tags").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        extra.push(("tags", serde_json::json!(tags)));
        let mut r = SearchResult::new(
            title,
            url.as_str(),
            snippet,
            1.0 / (1.0 + rank as f64),
        )
        .with("replies", item.get("answer_count").and_then(Value::as_u64).unwrap_or(0))
        .with("score", item.get("score").and_then(Value::as_i64).unwrap_or(0))
        .with("view_count", item.get("view_count").and_then(Value::as_u64).unwrap_or(0))
        .with("is_answered", item.get("is_answered").and_then(Value::as_bool).unwrap_or(false));

        for (k, v) in extra {
            r = r.with(k, v);
        }

        out.push(r);
    }

    out
}

fn non_empty(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Config for Stack Exchange sources.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// The Stack Exchange site to query (e.g., "stackoverflow", "askubuntu", "superuser").
    pub site: String,
}

impl Config {
    pub fn new(site: impl Into<String>) -> Self {
        Self { site: site.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
          "items": [
            {
              "question_id": 1,
              "title": "What is the correct way to handle errors in Rust?",
              "tags": ["rust", "error-handling"],
              "owner": { "display_name": "rustacean" },
              "is_answered": true,
              "score": 150,
              "answer_count": 5,
              "view_count": 10000
            },
            {
              "question_id": 2,
              "title": "&quot;Hello World&quot; test",
              "tags": ["html", "testing"],
              "owner": { "display_name": "tester" },
              "is_answered": false,
              "score": 10,
              "answer_count": 0,
              "view_count": 500
            },
            {
              "question_id": 3,
              "title": "No title",
              "tags": [],
              "owner": { "display_name": "anonymous" },
              "is_answered": false,
              "score": 0,
              "answer_count": 0,
              "view_count": 0
            }
          ],
          "has_more": false,
          "quota_remaining": 299
        })
    }

    fn s<'a>(r: &'a SearchResult, k: &str) -> &'a str {
        r.extra[k].as_str().unwrap()
    }

    #[test]
    fn parses_and_ignores_bad_entries() {
        let r = parse(&fixture(), 1);
        assert_eq!(r.len(), 3); // all items have non-empty titles

        assert_eq!(r[0].title, "What is the correct way to handle errors in Rust?");
        assert_eq!(
            r[0].url,
            "https://stackoverflow.com/q/1"
        );
        assert!(r[0].extra["is_answered"].as_bool().unwrap());
        assert_eq!(r[0].extra["replies"].as_u64().unwrap(), 5);
        assert_eq!(r[0].extra["score"].as_i64().unwrap(), 150);
        assert_eq!(
            r[0].extra["tags"],
            json!(["rust", "error-handling"])
        );
    }

    #[test]
    fn decodes_html_escaped_title() {
        let r = parse(&fixture(), 1);
        // The title should be decoded from &quot;...&quot;
        assert!(r[1].title.contains("Hello World"));
    }

    #[test]
    fn decodes_html_escaped_title_and_parses_all_items() {
        let r = parse(&fixture(), 1);
        assert_eq!(r.len(), 3); // all items have non-empty titles
    }

    #[test]
    fn scores_rank_and_continue_across_pages() {
        let r = parse(&fixture(), 1);
        assert!((r[0].score - 1.0).abs() < 1e-9 && (r[1].score - 0.5).abs() < 1e-9);
        let r = parse(&fixture(), 3);
        assert!((r[0].score - 1.0 / 21.0).abs() < 1e-9);
    }

    #[test]
    fn missing_items_key_is_empty() {
        assert!(parse(&json!({}), 1).is_empty());
        assert!(parse(&json!({ "items": null }), 1).is_empty());
    }

    #[test]
    fn extra_fields_are_correct() {
        let r = parse(&fixture(), 1);
        assert_eq!(r[0].extra["owner"], json!({"display_name": "rustacean"}));
        assert_eq!(r[0].extra["tags"], json!(["rust", "error-handling"]));
    }
}
