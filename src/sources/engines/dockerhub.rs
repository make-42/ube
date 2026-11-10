//! Docker Hub search, ported from SearXNG's `docker_hub.py`.
//!
//! Uses Docker Hub's JSON catalog endpoint
//! (`/api/search/v3/catalog/search`), so no HTML scraping is involved.
//!
//! Besides the usual fields, each result carries these `extra` keys:
//! `thumbnail`, `package_name`, `maintainer`, `publishedDate` (RFC 3339
//! string as returned by Docker Hub), `popularity` (e.g. "1000 pulls, 5 stars")
//! and `tags` (supported CPU architectures).

use crate::types::SearchResult;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};

const BASE_URL: &str = "https://hub.docker.com";
const PAGE_SIZE: u32 = 10;

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("ube/0.1")
            .build()
            .expect("failed to build Docker Hub HTTP client")
    })
}

/// Default entry point: `Source::new("dockerhub", dockerhub::dockerhub_pages)`.
pub async fn dockerhub_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let from = (page - 1) * PAGE_SIZE;

    let resp = client()
        .get(format!("{BASE_URL}/api/search/v3/catalog/search"))
        .query(&[
            ("query", q.as_str()),
            ("from", &from.to_string()),
            ("size", &PAGE_SIZE.to_string()),
        ])
        .header("accept", "application/json")
        .send()
        .await
        .context("Docker Hub request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read Docker Hub response body")?;

    if status.as_u16() == 429 {
        bail!("Docker Hub rate limited the request (HTTP 429)");
    }
    if !status.is_success() {
        let excerpt: String = body.chars().take(300).collect();
        bail!("Docker Hub returned HTTP {status}: {excerpt:?}");
    }

    let json: Value = serde_json::from_str(&body).context("Docker Hub returned invalid JSON")?;
    let results = parse(&json, page);

    if results.is_empty() && json.get("results").is_none() {
        tracing::warn!("dockerhub: no `results` key for {q:?} page {page} (API changed?)");
    }
    Ok(results)
}

// ---- JSON parsing ------------------------------------------------------------

/// Parse a catalog search response. Pure function, no network I/O.
pub fn parse(json: &Value, page: u32) -> Vec<SearchResult> {
    let items = json.get("results").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let mut out = Vec::new();

    for item in items {
        let Some(slug) = non_empty(item.get("slug")) else { continue };
        let name = non_empty(item.get("name")).unwrap_or(slug);

        let official = matches!(item.get("source").and_then(Value::as_str), Some("store" | "official"));
        let url = format!("{BASE_URL}{}{slug}", if official { "/_/" } else { "/r/" });

        let mut popularity = vec![format!("{} stars", count(item.get("star_count")).unwrap_or(0))];
        let mut architectures: Vec<String> = Vec::new();

        for plan in item.get("rate_plans").and_then(Value::as_array).into_iter().flatten() {
            let pulls = plan
                .get("repositories")
                .and_then(|r| r.get(0))
                .and_then(|r| count(r.get("pull_count")))
                .filter(|&n| n > 0);
            if let Some(n) = pulls {
                popularity.insert(0, format!("{n} pulls"));
            }
            for arch in plan.get("architectures").and_then(Value::as_array).into_iter().flatten() {
                if let Some(n) = non_empty(arch.get("name")) {
                    if !architectures.iter().any(|a| a == n) {
                        architectures.push(n.to_string());
                    }
                }
            }
        }

        // First result scores 1.0, second 0.5, ... continuing across pages.
        let rank = (page.max(1) - 1) as usize * PAGE_SIZE as usize + out.len();
        let mut r = SearchResult::new(
            name,
            url,
            non_empty(item.get("short_description")).unwrap_or(""),
            1.0 / (1.0 + rank as f64),
        )
        .with("package_name", name)
        .with("popularity", popularity.join(", "))
        .with("tags", architectures);

        if let Some(logo) = item.get("logo_url") {
            if let Some(t) = non_empty(logo.get("large")).or_else(|| non_empty(logo.get("small"))) {
                r = r.with("thumbnail", t);
            }
        }
        if let Some(m) = item.get("publisher").and_then(|p| non_empty(p.get("name"))) {
            r = r.with("maintainer", m);
        }
        if let Some(d) = non_empty(item.get("updated_at")).or_else(|| non_empty(item.get("created_at"))) {
            r = r.with("publishedDate", d);
        }
        r = r.icon(format!("{BASE_URL}/favicon.ico"));
        out.push(r);
    }
    out
}

fn non_empty(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Docker Hub returns counts as numbers, but tolerate numeric strings.
fn count(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

// ---- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
          "total": 3,
          "results": [
            {
              "id": "nginx", "name": "nginx", "slug": "nginx", "source": "store",
              "short_description": "Official build of Nginx.",
              "star_count": 21000,
              "logo_url": { "large": "https://img/nginx-l.png", "small": "https://img/nginx-s.png" },
              "publisher": { "id": "docker", "name": "Docker Official Images" },
              "updated_at": "2026-11-01T10:00:00Z",
              "created_at": "2014-06-01T00:00:00Z",
              "rate_plans": [
                { "repositories": [{ "pull_count": 1000000 }],
                  "architectures": [{ "name": "amd64" }, { "name": "arm64" }, { "name": "" }] },
                { "repositories": [{ "pull_count": "5000" }],
                  "architectures": [{ "name": "amd64" }, { "name": "s390x" }] }
              ]
            },
            {
              "name": "bitnami/nginx", "slug": "bitnami/nginx", "source": "community",
              "star_count": 7,
              "logo_url": { "small": "https://img/b-s.png" },
              "publisher": { "name": "Bitnami" },
              "created_at": "2020-01-01T00:00:00Z",
              "rate_plans": []
            },
            { "name": "no-slug" }
          ]
        })
    }

    fn s<'a>(r: &'a SearchResult, k: &str) -> &'a str {
        r.extra[k].as_str().unwrap()
    }

    #[test]
    fn parses_official_image() {
        let r = parse(&fixture(), 1);
        assert_eq!(r.len(), 2); // the item without a slug is skipped

        assert_eq!(r[0].title, "nginx");
        assert_eq!(r[0].url, "https://hub.docker.com/_/nginx");
        assert_eq!(r[0].description, "Official build of Nginx.");
        assert_eq!(s(&r[0], "thumbnail"), "https://img/nginx-l.png");
        assert_eq!(s(&r[0], "maintainer"), "Docker Official Images");
        assert_eq!(s(&r[0], "publishedDate"), "2026-11-01T10:00:00Z");
        assert_eq!(s(&r[0], "package_name"), "nginx");
        // like SearXNG, later plans are inserted first
        assert_eq!(s(&r[0], "popularity"), "5000 pulls, 1000000 pulls, 21000 stars");
        assert_eq!(r[0].extra["tags"], json!(["amd64", "arm64", "s390x"]));
    }

    #[test]
    fn parses_community_image_with_fallbacks() {
        let r = parse(&fixture(), 1);
        assert_eq!(r[1].url, "https://hub.docker.com/r/bitnami/nginx");
        assert_eq!(r[1].description, "");
        assert_eq!(s(&r[1], "thumbnail"), "https://img/b-s.png"); // falls back to `small`
        assert_eq!(s(&r[1], "publishedDate"), "2020-01-01T00:00:00Z"); // falls back to created_at
        assert_eq!(s(&r[1], "popularity"), "7 stars");
        assert_eq!(r[1].extra["tags"], json!([]));
    }

    #[test]
    fn scores_rank_and_continue_across_pages() {
        let r = parse(&fixture(), 1);
        assert!((r[0].score - 1.0).abs() < 1e-9 && (r[1].score - 0.5).abs() < 1e-9);
        let r = parse(&fixture(), 3);
        assert!((r[0].score - 1.0 / 21.0).abs() < 1e-9);
    }

    #[test]
    fn missing_results_key_is_empty() {
        assert!(parse(&json!({}), 1).is_empty());
        assert!(parse(&json!({ "results": null }), 1).is_empty());
    }
}
