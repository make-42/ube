//! Google web search, ported from SearXNG's `google.py`.
//!
//! Uses headless Chrome via the Chrome DevTools Protocol (CDP) to scrape
//! the modern JavaScript-rendered Google search page.
//!
//! This implementation navigates to the Google search page and executes
//! JavaScript to fill the form and trigger results.
//!
//! Pages are paginated via the `start` query parameter (0-based). Page 1 = start=0, page 2 = start=10.
//!
//! Notes:
//! - Requires Chrome binaries (the headless_chrome crate manages this)
//! - Higher resource usage than reqwest/wreq
//! - May be blocked by Google's anti-bot measures
//!
//! What is NOT verified:
//! - Live scraping against Google (blocked/unverified)

use crate::types::SearchResult;
use anyhow::{bail, Context, Result};
use scraper::{ElementRef, Html, Selector};
use std::sync::OnceLock;
use url::Url;

const CHROME_TIMEOUT_MS: u64 = 15_000;

fn chrome_browser() -> &'static headless_chrome::Browser {
    static BROWSER: OnceLock<headless_chrome::Browser> = OnceLock::new();
    BROWSER.get_or_init(|| headless_chrome::Browser::default().expect("failed to launch headless Chrome"))
}

const SEL_RESULT: &str = r#"[class*="XMnDkb"]"#;
const SEL_TITLE: &str = r#"span[class*="dF1cRf"]"#;
const SEL_CONTENT: &str = r#"div[class*="tF5Cvf"]"#;
const SEL_THUMBNAIL: &str = r#"div[class*="w46CJ"] img"#;

pub async fn google_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    google_with(crate::sources::foundations::google::GoogleConfig::default(), q, page).await
}

pub async fn google_with(
    cfg: crate::sources::foundations::google::GoogleConfig,
    q: String,
    page: u32,
) -> Result<Vec<SearchResult>> {
    let start = (page - 1) * 10;
    let request_url = crate::sources::foundations::google::build_google_url(&cfg, &q, page, cfg.tbm.as_deref())?;
    let request_url = request_url.to_string();

    let browser = chrome_browser();
    let mut tab = browser.new_tab().context("failed to create new tab")?;

    // Navigate to the search page with the query in the URL
    tab.navigate_to(request_url.as_str()).context("Chrome navigation failed")?;
    tab.wait_until_navigated().context("Chrome navigation timed out")?;

    // Get the results page
    let body = tab.get_content().context("failed to get results page content")?;

    // Check for common block indicators
    if body.len() < 5000 {
        tracing::warn!("Google page content ({} bytes):\n{}", body.len(), body);
        bail!("Google returned an unusually small response ({} bytes). Likely blocked or CAPTCHA.", body.len());
    }

    let (results, _suggestions) = parse(&body);

    if results.is_empty() {
        tracing::warn!(
            "google: 0 results parsed for {q:?} page {page} (start={start}) \
             (no matches, or Google's layout changed)"
        );
    }

    Ok(results)
}

fn parse(body: &str) -> (Vec<SearchResult>, Vec<String>) {
    let dom = Html::parse_document(body);
    let mut results = Vec::new();

    for item in dom.select(&Selector::parse(SEL_RESULT).unwrap()) {
        let Some(title_el) = item.select(&Selector::parse(SEL_TITLE).unwrap()).next() else { continue; };
        let title = text(title_el);

        if title.is_empty() { continue; }

        let Some(link_el) = item.select(&Selector::parse(r#"[class*="uWmqRe"]"#).unwrap()).next() else { continue; };
        let Some(href) = link_el.value().attr("href") else { continue; };
        let Some(url) = unwrap_google_url(href) else { continue; };

        let content = item.select(&Selector::parse(SEL_CONTENT).unwrap())
            .next()
            .map(|e| e.text().flat_map(str::split_whitespace).collect::<Vec<_>>())
            .unwrap_or_default()
            .join(" ");

        let thumbnail = item.select(&Selector::parse(SEL_THUMBNAIL).unwrap())
            .next()
            .and_then(|i| i.value().attr("src"))
            .map(|s| s.to_string());

        let rank = results.len();

        let mut result = SearchResult::new(title, url.clone(), content, 1.0 / (1.0 + rank as f64));
        if let Some(icon) = favicon(&url) { result = result.icon(icon); }
        if let Some(src) = thumbnail { result = result.with("thumbnail", src); }

        results.push(result);
    }

    (results, Vec::new())
}

fn text(el: ElementRef<'_>) -> String {
    el.text().flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ")
}

fn unwrap_google_url(raw: &str) -> Option<String> {
    if raw.starts_with("/url?") {
        let url = Url::parse(&format!("https://www.google.com{raw}")).ok()?;
        return url.query_pairs().find(|(k, _)| k == "q").map(|(_, v)| v.into_owned());
    }
    raw.starts_with("http").then(|| raw.to_string())
}

fn favicon(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    Some(format!("{}://{}/favicon.ico", url.scheme(), url.host_str()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<!DOCTYPE html>
<html>
<head><title>Google Search</title></head>
<body>
<div class="XMnDkb">
  <a class="uWmqRe" href="/url?q=https%3A%2F%2Fexample.com%2Fa%3Fx%3D1">
    <span class="dF1cRf">Example   Title</span>
  </a>
  <div class="tF5Cvf">Some <b>snippet</b> text</div>
  <div class="w46CJ">
    <img src="https://encrypted-tbn0.gstatic.com/images?q=1">
  </div>
</div>
<div class="XMnDkb">
  <a class="uWmqRe" href="/url?q=https://www.rust-lang.org/">
    <span class="dF1cRf">Rust</span>
  </a>
</div>
</body>
</html>"#;

    #[test]
    fn parses_results_and_suggestions() {
        let (results, _) = parse(FIXTURE);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Title");
        assert_eq!(results[0].url, "https://example.com/a?x=1");
        assert_eq!(results[0].description, "Some snippet text");
        assert_eq!(results[0].icon.as_deref(), Some("https://example.com/favicon.ico"));
        assert!(results[0].extra.contains_key("thumbnail"));
        assert_eq!(results[1].url, "https://www.rust-lang.org/");
        assert!((results[0].score - 1.0).abs() < 1e-9 && (results[1].score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn scores_continue_across_pages() {
        let (results, _) = parse(FIXTURE);
        assert!((results[0].score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unwraps_urls() {
        assert_eq!(unwrap_google_url("/url?q=https%3A%2F%2Fa.b%2Fc&sa=U").as_deref(), Some("https://a.b/c"));
        assert_eq!(unwrap_google_url("https://a.b/").as_deref(), Some("https://a.b/"));
        assert_eq!(unwrap_google_url("/search?q=x"), None);
    }
}
