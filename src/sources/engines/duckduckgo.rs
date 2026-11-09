
use crate::types::SearchResult;
use anyhow::{bail, Context, Result};
use reqwest::{
    header::{ACCEPT, USER_AGENT},
    Client, StatusCode,
};
use scraper::{ElementRef, Html, Selector};
use std::{
    sync::OnceLock,
    time::Duration,
};
use url::Url;

// Ported from Djarvur/ddg-search:
// internal/search/client.go
// internal/search/search.go
// internal/search/parser.go

const DDG_URL: &str = "https://html.duckduckgo.com/html";

const UA: &str = "Mozilla/5.0 (compatible; ddg-search/1.0)";

const ACCEPT_HEADER: &str =
    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

const MAX_RETRIES: u32 = 3;
const MAX_RESULTS: usize = 10;

const BASE_DELAY_MS: u64 = 1_000;
const MAX_DELAY_MS: u64 = 30_000;
const BACKOFF_MULTIPLIER: u64 = 2;
const JITTER_MAX_MS: u32 = 500;

const SEL_RESULT_LINK: &str = ".result__a";
const SEL_SNIPPET: &str = ".result__snippet";

struct Selectors {
    result_link: Selector,
    snippet: Selector,
}

fn selectors() -> &'static Selectors {
    static S: OnceLock<Selectors> = OnceLock::new();

    S.get_or_init(|| {
        let p = |s| Selector::parse(s).expect("valid CSS selector");

        Selectors {
            result_link: p(SEL_RESULT_LINK),
            snippet: p(SEL_SNIPPET),
        }
    })
}

fn client() -> &'static Client {
    static C: OnceLock<Client> = OnceLock::new();

    C.get_or_init(|| {
        Client::builder()
            // Use reqwest's default redirect handling, like Resty's default
            // HTTP client behavior in the Go implementation.
            .build()
            .expect("failed to build DuckDuckGo HTTP client")
    })
}

#[derive(Clone, Debug)]
pub struct DuckDuckGoConfig {
    pub kl: String,
    pub df: Option<&'static str>,

    // Retained for compatibility with the previous Rust config.
    // Djarvur's client does not send Accept-Language.
    pub accept_language: String,
}

impl Default for DuckDuckGoConfig {
    fn default() -> Self {
        Self {
            kl: "us-en".into(),
            df: None,
            accept_language: "en-US,en;q=0.7".into(),
        }
    }
}

/// Default entry point:
/// Source::new("duckduckgo", engines::duckduckgo::duckduckgo_pages)
pub async fn duckduckgo_pages(
    q: String,
    page: u32,
) -> Result<Vec<SearchResult>> {
    duckduckgo_with(DuckDuckGoConfig::default(), q, page).await
}

pub async fn duckduckgo_with(
    cfg: DuckDuckGoConfig,
    q: String,
    page: u32,
) -> Result<Vec<SearchResult>> {
    if q.trim().is_empty() {
        return Ok(Vec::new());
    }

    if q.chars().count() >= 500 {
        bail!("duckduckgo does not accept queries of 500+ characters");
    }

    // Djarvur's implementation only fetches the HTML search page.
    // It does not implement vqd-based pagination.
    if page > 1 {
        return Ok(Vec::new());
    }

    let body = fetch(&cfg, &q).await?;
    let results = parse(&body);

    if results.is_empty() {
        tracing::warn!(
            "duckduckgo: zero results parsed for {:?}; \
             the query may have no matches or the HTML layout may have changed",
            q
        );
    }

    Ok(results)
}

// ---- request and retry -----------------------------------------------------

fn retry_delay(attempt: u32) -> Duration {
    let multiplier = BACKOFF_MULTIPLIER.saturating_pow(attempt);

    let base_ms = BASE_DELAY_MS
        .saturating_mul(multiplier)
        .min(MAX_DELAY_MS);

    // Go implementation: random jitter from 0 to 500 ms.
    let jitter_ms =
        (rand::random::<u32>() % (JITTER_MAX_MS + 1)) as u64;

    Duration::from_millis(base_ms + jitter_ms)
}

async fn wait_before_retry(attempt: u32, reason: &str) {
    let delay = retry_delay(attempt);

    tracing::warn!(
        "duckduckgo: {}. Waiting {:?} before retry {}/{}",
        reason,
        delay,
        attempt + 2,
        MAX_RETRIES + 1
    );

    tokio::time::sleep(delay).await;
}

fn is_rate_limited(status: StatusCode) -> bool {
    status == StatusCode::ACCEPTED
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

async fn fetch(
    cfg: &DuckDuckGoConfig,
    q: &str,
) -> Result<String> {
    // Djarvur uses GET query parameters, not a POST form.
    let mut params: Vec<(String, String)> = vec![
        ("q".into(), q.into()),
        ("p".into(), "-1".into()),
    ];

    if !cfg.kl.is_empty() {
        params.push(("kl".into(), cfg.kl.clone()));
    }

    if let Some(df) = cfg.df {
        params.push(("df".into(), df.into()));
    }

    for attempt in 0..=MAX_RETRIES {
        let response = client()
            .get(DDG_URL)
            .header(USER_AGENT, UA)
            .header(ACCEPT, ACCEPT_HEADER)
            .query(&params)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,

            Err(error) => {
                if attempt == MAX_RETRIES {
                    return Err(error)
                        .context("duckduckgo request failed after retries");
                }

                wait_before_retry(
                    attempt,
                    &format!("network error: {error}"),
                )
                .await;

                continue;
            }
        };

        let status = response.status();

        // HTTP 202 is especially important:
        // DDG uses it for its automated-traffic/rate-limit response.
        if is_rate_limited(status) {
            if attempt == MAX_RETRIES {
                bail!(
                    "duckduckgo remained rate-limited after {} attempts \
                     (last HTTP status: {})",
                    MAX_RETRIES + 1,
                    status
                );
            }

            wait_before_retry(
                attempt,
                &format!("HTTP {status}"),
            )
            .await;

            continue;
        }

        let body = match response.text().await {
            Ok(body) => body,

            Err(error) => {
                if attempt == MAX_RETRIES {
                    return Err(error)
                        .context("reading duckduckgo response after retries");
                }

                wait_before_retry(
                    attempt,
                    &format!("failed to read response: {error}"),
                )
                .await;

                continue;
            }
        };

        return Ok(body);
    }

    unreachable!("retry loop always returns or continues")
}

// ---- parsing ---------------------------------------------------------------

fn parse(body: &str) -> Vec<SearchResult> {
    let s = selectors();
    let dom = Html::parse_document(body);

    let mut results = Vec::new();

    // Djarvur's parser searches .result__a directly.
    for a in dom.select(&s.result_link) {
        if results.len() >= MAX_RESULTS {
            break;
        }

        let title = text(a);

        let Some(href) = a.value().attr("href") else {
            continue;
        };

        let Some(url) = clean_url(href) else {
            continue;
        };

        if title.is_empty() {
            continue;
        }

        let snippet = snippet_for(a, s);

        let rank = results.len();
        let mut result = SearchResult::new(
            title,
            url.clone(),
            snippet,
            1.0 / (1.0 + rank as f64),
        );

        if let Some(icon) = favicon(&url) {
            result = result.icon(icon);
        }

        results.push(result);
    }

    results
}

fn snippet_for(
    a: ElementRef<'_>,
    selectors: &Selectors,
) -> String {
    // Match the Go implementation:
    // anchor -> .result__title -> grandparent -> .result__snippet

    let Some(parent_node) = a.parent() else {
        return String::new();
    };

    let Some(parent) = ElementRef::wrap(parent_node) else {
        return String::new();
    };

    let is_result_title = parent
        .value()
        .attr("class")
        .is_some_and(|classes| {
            classes
                .split_whitespace()
                .any(|class| class == "result__title")
        });

    if !is_result_title {
        return String::new();
    }

    let Some(grandparent_node) = parent.parent() else {
        return String::new();
    };

    let Some(grandparent) = ElementRef::wrap(grandparent_node) else {
        return String::new();
    };

    grandparent
        .select(&selectors.snippet)
        .next()
        .map(text)
        .unwrap_or_default()
}

fn text(el: ElementRef<'_>) -> String {
    el.text()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract the actual URL from DuckDuckGo's redirect URL.
/// Djarvur uses the `uddg` query parameter when present.
fn clean_url(href: &str) -> Option<String> {
    let absolute = match href.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => href.to_string(),
    };

    let url = Url::parse(&absolute).ok()?;

    let target = if href.contains("uddg=") {
        url.query_pairs()
            .find(|(key, _)| key == "uddg")
            .map(|(_, value)| value.into_owned())
            .unwrap_or(absolute)
    } else {
        absolute
    };

    let target_url = Url::parse(&target).ok()?;

    matches!(target_url.scheme(), "http" | "https")
        .then_some(target)
}

fn favicon(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;

    Some(format!(
        "{}://{}/favicon.ico",
        url.scheme(),
        url.host_str()?
    ))
}

// ---- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
    <html><body>
      <div id="links">
        <div class="result result--ad">
          <h2 class="result__title">
            <a href="//duckduckgo.com/y.js?ad=1">Ad</a>
          </h2>
        </div>

        <div class="result results_links web-result">
          <h2 class="result__title">
            <a class="result__a"
               href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fx%3D1&amp;rut=abc">
              Example Title
            </a>
          </h2>
          <a class="result__snippet">
            A <b>snippet</b> here
          </a>
        </div>

        <div class="result web-result">
          <h2 class="result__title">
            <a class="result__a" href="https://www.rust-lang.org/">
              Rust
            </a>
          </h2>
        </div>
      </div>
    </body></html>
    "#;

    #[test]
    fn parses_result_links_and_snippets() {
        let results = parse(FIXTURE);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Title");
        assert_eq!(
            results[0].url,
            "https://example.com/a?x=1"
        );
        assert_eq!(
            results[0].description,
            "A snippet here"
        );
        assert_eq!(
            results[1].url,
            "https://www.rust-lang.org/"
        );
    }

    #[test]
    fn unwraps_redirect_urls() {
        assert_eq!(
            clean_url(
                "//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.b%2Fc&rut=x"
            )
            .as_deref(),
            Some("https://a.b/c")
        );
    }

    #[test]
    fn accepts_direct_urls() {
        assert_eq!(
            clean_url("https://example.com/").as_deref(),
            Some("https://example.com/")
        );

        assert_eq!(clean_url("/relative"), None);
    }
}
