//! GitHub search, ported from SearXNG's `github.py`.
//!
//! Endpoint: `https://api.github.com/search/repositories?sort=stars&order=desc&q`
//! Paging:   1-based `page`, 10 results per page.
//! Notes:    Anonymous rate limit is 10 requests/minute; treat 403/429 as Err.
//!           Optional `GITHUB_TOKEN` env var for authenticated requests.

use crate::types::SearchResult;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{sync::OnceLock, time::Duration};

const BASE_URL: &str = "https://api.github.com/search/repositories";
const PAGE_SIZE: u32 = 10;

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("ube/0.1")
            .build()
            .expect("failed to build GitHub HTTP client")
    })
}

/// Default entry point: `Source::new("github", github::github_pages)`.
pub async fn github_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let resp = client()
        .get(format!("{BASE_URL}?sort=stars&order=desc&q={q}"))
        .query(&[("page", &page.to_string()), ("per_page", &PAGE_SIZE.to_string())])
        .header("Accept", "application/vnd.github.v3+json")
        .header("Accept", "application/vnd.github.preview+json")
        .send()
        .await
        .context("GitHub request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read GitHub response body")?;

    if status.as_u16() == 403 || status.as_u16() == 429 {
        bail!("GitHub blocked the request (HTTP {status}) - rate limit or abuse protection");
    }
    if !status.is_success() {
        let excerpt: String = body.chars().take(300).collect();
        bail!("GitHub returned HTTP {status}: {excerpt:?}");
    }

    let json: Value = serde_json::from_str(&body).context("GitHub returned invalid JSON")?;
    let results = parse(&json, page);

    if results.is_empty() && json.get("items").is_none() {
        tracing::warn!("github: no `items` key for {q:?} page {page} (API changed?)");
    }
    Ok(results)
}

/// Parse a GitHub search response. Pure function, no network I/O.
pub fn parse(json: &Value, page: u32) -> Vec<SearchResult> {
    let items = json.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let mut out = Vec::new();

    for item in items {
        let Some(full_name) = non_empty(item.get("full_name")) else { continue };
        let url = item.get("html_url").and_then(Value::as_str).filter(|s| !s.is_empty());
        let url = url.map(|u| u.to_string()).unwrap_or_else(|| format!("https://github.com/{full_name}"));
        let title = full_name.to_string();
        let description = non_empty(item.get("description")).unwrap_or("");

        let mut content = Vec::new();
        if let Some(lang) = item.get("language").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            content.push(lang.to_string());
        }
        if !description.is_empty() {
            content.push(description.to_string());
        }
        let content_str = if content.is_empty() { "" } else { &content.join(" / ") };

        let thumbnail = item
            .get("owner")
            .and_then(|o| o.get("avatar_url").and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // License
        let license_name = if let Some(lic) = item.get("license").and_then(|l| l.as_object()) {
            let spdx_id = lic.get("spdx_id").and_then(Value::as_str);
            if let Some(spdx) = spdx_id {
                Some(spdx.to_string())
            } else {
                None
            }
        } else {
            None
        };

        let license_url = license_name.as_ref().map(|s| format!("https://spdx.org/licenses/{s}.html"));

        // Popularity (stargazers_count)
        let stars = item
            .get("stargazers_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);

        // Maintainer
        let maintainer = item.get("owner").and_then(|o| o.get("login").and_then(Value::as_str)).map(|s| s.to_string());

        // Homepage
        let homepage = item.get("homepage").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| s.to_string());

        // Clone URL
        let clone_url = item.get("clone_url").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| s.to_string());

        // Topics
        let topics = item.get("topics").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        });

        // Date (updated_at or created_at)
        let updated_at = item.get("updated_at").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let created_at = item.get("created_at").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| s.to_string());

        let rank = (page.max(1) - 1) as usize * PAGE_SIZE as usize + out.len();
        let r = SearchResult::new(title, url.as_str(), content_str, 1.0 / (1.0 + rank as f64))
            .with("thumbnail", thumbnail)
            .with("package_name", full_name.to_string())
            .with("popularity", format!("{} stars", stars))
            .with("maintainer", maintainer)
            .with("homepage", homepage)
            .with("clone_url", clone_url)
            .with("topics", topics)
            .with("license_name", license_name)
            .with("license_url", license_url)
            .with("updated_at", updated_at)
            .with("created_at", created_at);

        out.push(r);
    }
    out
}

fn non_empty(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
          "total_count": 2,
          "incomplete_results": false,
          "items": [
            {
              "name": "rea",
              "full_name": "morluto/rea",
              "html_url": "https://github.com/morluto/rea",
              "description": "Reverse engineer anything with agents",
              "language": "TypeScript",
              "stargazers_count": 459792,
              "owner": {
                "login": "morluto",
                "avatar_url": "https://avatars.githubusercontent.com/u/76467478?v=4"
              },
              "license": { "spdx_id": "MIT", "name": "MIT License", "url": "https://api.github.com/licenses/mit" },
              "homepage": "https://rea.tools",
              "clone_url": "https://github.com/morluto/rea.git",
              "topics": ["agents", "reverse-engineering"],
              "updated_at": "2026-11-14T23:24:24Z"
            },
            {
              "name": "hello-algo",
              "full_name": "krahets/hello-algo",
              "html_url": "https://github.com/krahets/hello-algo",
              "description": "《Hello 算法》：动画图解算法教程",
              "language": "Python",
              "stargazers_count": 132320,
              "owner": {
                "login": "krahets",
                "avatar_url": "https://avatars.githubusercontent.com/u/26993056?v=4"
              },
              "license": { "spdx_id": "NOASSERTION", "name": "Other", "url": null },
              "topics": ["algorithm", "data-structures"],
              "updated_at": "2026-11-14T20:55:47Z"
            },
            { "name": "no-full-name" }
          ]
        })
    }

    fn s<'a>(r: &'a SearchResult, k: &str) -> &'a str {
        r.extra[k].as_str().unwrap()
    }

    #[test]
    fn parses_and_skips_bad_entries() {
        let r = parse(&fixture(), 1);
        assert_eq!(r.len(), 2); // the item without full_name is skipped
        assert_eq!(r[0].title, "morluto/rea");
        assert_eq!(r[0].url, "https://github.com/morluto/rea");
        assert_eq!(r[0].description, "TypeScript / Reverse engineer anything with agents");
        assert_eq!(r[0].extra["thumbnail"], json!("https://avatars.githubusercontent.com/u/76467478?v=4"));
        assert_eq!(r[0].extra["package_name"], "morluto/rea");
        assert_eq!(r[0].extra["popularity"], "459792 stars");
        assert_eq!(r[0].extra["maintainer"], "morluto");
        assert_eq!(r[0].extra["homepage"], "https://rea.tools");
        assert_eq!(r[0].extra["clone_url"], "https://github.com/morluto/rea.git");
        assert_eq!(r[0].extra["license_name"], "MIT");
        assert_eq!(r[0].extra["license_url"], "https://spdx.org/licenses/MIT.html");
        assert_eq!(r[0].extra["topics"], json!(["agents", "reverse-engineering"]));
        assert_eq!(r[0].extra["updated_at"], "2026-11-14T23:24:24Z");
    }

    #[test]
    fn parses_community_repo_without_license() {
        let r = parse(&fixture(), 1);
        assert_eq!(r[1].title, "krahets/hello-algo");
        assert_eq!(r[1].extra["popularity"], "132320 stars");
        assert_eq!(r[1].extra["maintainer"], "krahets");
        assert_eq!(r[1].extra["license_name"], "NOASSERTION");
        assert_eq!(r[1].extra["license_url"], "https://spdx.org/licenses/NOASSERTION.html");
    }

    #[test]
    fn scores_continue_across_pages() {
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
    fn empty_query_returns_empty() {
        // GitHub API returns empty items for empty query
        let r = parse(&json!({"items": []}), 1);
        assert!(r.is_empty());
    }
}
