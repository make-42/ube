
//! DuckDuckGo HTML search engine.
//!
//! Based on Djarvur/ddg-search's HTTP client conventions:
//! - Fixed User-Agent and Accept headers.
//! - Retries network errors, HTTP 202, HTTP 429 and HTTP 5xx.
//! - Exponential backoff with jitter.
//!
//! Pagination follows DuckDuckGo's no-JavaScript HTML form protocol:
//! - Page 1 obtains a vqd token.
//! - Page 2+ sends that token with the continuation parameters.
//! - Page 1 results and vqd are cached together.
//!
//! Page sizes:
//! - Page 1: 10 results.
//! - Page 2+: 15 results per page.

use crate::types::SearchResult;
use anyhow::{anyhow, bail, Context, Result};
use moka::future::Cache;
use reqwest::{
   header::{
       ACCEPT, ACCEPT_LANGUAGE, COOKIE, REFERER, USER_AGENT,
   },
   Client, RequestBuilder, StatusCode,
};
use scraper::{ElementRef, Html, Selector};
use std::{
   sync::{Arc, OnceLock},
   time::{Duration, Instant},
};
use url::Url;

// ---- constants -------------------------------------------------------------

const DDG_URL: &str = "https://html.duckduckgo.com/html/";

/// Must stay fixed because DDG's vqd token is associated with the UA.
const UA: &str = "Mozilla/5.0 (compatible; ddg-search/1.0)";

const ACCEPT_HEADER: &str =
   "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 1_000;
const MAX_DELAY_MS: u64 = 30_000;
const JITTER_MAX_MS: u32 = 500;

const PAGE1_SIZE: usize = 10;
const NEXT_PAGE_SIZE: usize = 15;

// CSS translations of SearXNG's result selectors.
const SEL_RESULT: &str = r#"#links > div[class*="web-result"]"#;
const SEL_TITLE: &str = "h2 a";
const SEL_SNIPPET: &str = "a.result__snippet";
const SEL_VQD: &str = r#"input[name="vqd"]"#;
const SEL_CAPTCHA: &str = "form#challenge-form";

// ---- selectors -------------------------------------------------------------

struct Selectors {
   result: Selector,
   title: Selector,
   snippet: Selector,
   vqd: Selector,
   captcha: Selector,
}

fn selectors() -> &'static Selectors {
   static S: OnceLock<Selectors> = OnceLock::new();

   S.get_or_init(|| {
       let p = |s| Selector::parse(s).expect("valid CSS selector");

       Selectors {
           result: p(SEL_RESULT),
           title: p(SEL_TITLE),
           snippet: p(SEL_SNIPPET),
           vqd: p(SEL_VQD),
           captcha: p(SEL_CAPTCHA),
       }
   })
}

// ---- HTTP client -----------------------------------------------------------

fn client() -> &'static Client {
   static C: OnceLock<Client> = OnceLock::new();

   C.get_or_init(|| {
       Client::builder()
           .timeout(Duration::from_secs(15))
           // A 303 can be a DDG !bang redirect. Do not follow it.
           .redirect(reqwest::redirect::Policy::none())
           .build()
           .expect("failed to build DuckDuckGo HTTP client")
   })
}

// ---- configuration ---------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DuckDuckGoConfig {
   /// Region, e.g. "wt-wt", "us-en", "fr-fr", or "de-de".
   pub kl: String,

   /// Time filter: "d", "w", "m", or "y".
   pub df: Option<&'static str>,

   pub accept_language: String,
}

impl Default for DuckDuckGoConfig {
   fn default() -> Self {
       Self {
           kl: "wt-wt".into(),
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

// ---- pagination cache ------------------------------------------------------

struct Page1 {
   results: Vec<SearchResult>,
   vqd: Option<String>,
   at: Instant,
}

/// Store page 1 and its token together.
///
/// A page-1 result set is considered fresh for 30 seconds.
/// The token can be reused for up to one hour.
fn page1_cache() -> &'static Cache<String, Arc<Page1>> {
   static C: OnceLock<Cache<String, Arc<Page1>>> = OnceLock::new();

   C.get_or_init(|| {
       Cache::builder()
           .max_capacity(2_000)
           .time_to_live(Duration::from_secs(3600))
           .build()
   })
}

fn cache_key(cfg: &DuckDuckGoConfig, q: &str) -> String {
   format!(
       "{}|{}|{}|{}",
       cfg.kl,
       cfg.df.unwrap_or(""),
       cfg.accept_language,
       q
   )
}

/// Fetch page 1, sharing concurrent requests for the same query.
///
/// `fresh = true` is used by a page-1 request. It refreshes results older
/// than 30 seconds.
///
/// `fresh = false` is used by page 2+. It only needs a valid cached token.
async fn page1(
   cfg: &DuckDuckGoConfig,
   q: &str,
   fresh: bool,
) -> Result<Arc<Page1>> {
   let key = cache_key(cfg, q);
   let cache = page1_cache();

   if fresh {
       if let Some(cached) = cache.get(&key).await {
           if cached.at.elapsed() < Duration::from_secs(30) {
               return Ok(cached);
           }

           cache.invalidate(&key).await;
       }
   }

   cache
       .try_get_with(key, async {
           let parsed = fetch(cfg, q, 1, None).await?;

           Ok::<_, anyhow::Error>(Arc::new(Page1 {
               results: parsed.results,
               vqd: parsed.vqd,
               at: Instant::now(),
           }))
       })
       .await
       .map_err(|e| anyhow!("{e}"))
}

// ---- public search entry point --------------------------------------------

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

   let page = page.max(1);

   if page == 1 {
       let p1 = page1(&cfg, &q, true).await?;

       if p1.results.is_empty() {
           tracing::warn!(
               "duckduckgo: zero results parsed for {:?} \
                (no matches, a bang redirect, or layout change)",
               q
           );
       }

       return Ok(p1.results.clone());
   }

   // Page 2+ requires the token obtained from page 1.
   // If page 2 is requested first, this fetches page 1 automatically.
   let p1 = page1(&cfg, &q, false).await?;

   let vqd = p1
       .vqd
       .as_deref()
       .context("DuckDuckGo did not provide a vqd token on page 1")?;

   let parsed = fetch(&cfg, &q, page, Some(vqd)).await?;

   // Keep the latest token returned by DDG.
   if let Some(new_vqd) = &parsed.vqd {
       let updated = Page1 {
           results: p1.results.clone(),
           vqd: Some(new_vqd.clone()),
           at: p1.at,
       };

       page1_cache()
           .insert(cache_key(&cfg, &q), Arc::new(updated))
           .await;
   }

   Ok(parsed.results)
}

// ---- query helpers ---------------------------------------------------------

/// Results start after the first 10, then arrive in blocks of 15.
///
/// Page 1 = 0
/// Page 2 = 10
/// Page 3 = 25
/// Page 4 = 40
fn offset(page: u32) -> u32 {
   if page <= 1 {
       0
   } else {
       10u32.saturating_add(page.saturating_sub(2).saturating_mul(15))
   }
}

fn page_size(page: u32) -> usize {
   if page <= 1 {
       PAGE1_SIZE
   } else {
       NEXT_PAGE_SIZE
   }
}

/// Quote bang-like words so DDG does not redirect away from search results.
///
/// This is intentionally conservative: we do not maintain DDG's bang database.
fn quote_bangs(q: &str) -> String {
   q.split_whitespace()
       .map(|word| {
           if word.len() > 1 && word.starts_with('!') {
               format!("'{word}'")
           } else {
               word.to_string()
           }
       })
       .collect::<Vec<_>>()
       .join(" ")
}

fn cookie_header(cfg: &DuckDuckGoConfig) -> Option<String> {
   let mut cookies = Vec::new();

   if cfg.kl != "wt-wt" {
       cookies.push(format!("kl={}", cfg.kl));
   }

   if let Some(df) = cfg.df {
       cookies.push(format!("df={df}"));
   }

   if cookies.is_empty() {
       None
   } else {
       Some(cookies.join("; "))
   }
}

// ---- retries ---------------------------------------------------------------

fn is_retryable_status(status: StatusCode) -> bool {
   status == StatusCode::ACCEPTED
       || status == StatusCode::TOO_MANY_REQUESTS
       || status.is_server_error()
}

fn retry_delay(attempt: u32) -> Duration {
   let multiplier = 2u64.saturating_pow(attempt);

   let base_ms = BASE_DELAY_MS
       .saturating_mul(multiplier)
       .min(MAX_DELAY_MS);

   // Djarvur-style 0–500ms random jitter.
   let jitter_ms = (rand::random::<u32>() % (JITTER_MAX_MS + 1)) as u64;

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

/// Execute a request with Djarvur-style retry handling.
///
/// Retries:
/// - Network errors
/// - HTTP 202
/// - HTTP 429
/// - HTTP 5xx
///
/// Returns the status and body. HTTP 303 is returned without following it.
async fn send_with_retries<F>(
   mut build: F,
) -> Result<(StatusCode, String)>
where
   F: FnMut() -> RequestBuilder,
{
   for attempt in 0..=MAX_RETRIES {
       let response = match build().send().await {
           Ok(response) => response,

           Err(error) => {
               if attempt == MAX_RETRIES {
                   return Err(error)
                       .context("DuckDuckGo request failed after retries");
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

       if status == StatusCode::SEE_OTHER {
           return Ok((status, String::new()));
       }

       if is_retryable_status(status) {
           if attempt == MAX_RETRIES {
               bail!(
                   "DuckDuckGo remained rate-limited or unavailable \
                    after {} attempts (last HTTP status: {})",
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

       if !status.is_success() {
           bail!("DuckDuckGo returned HTTP {status}");
       }

       let body = match response.text().await {
           Ok(body) => body,

           Err(error) => {
               if attempt == MAX_RETRIES {
                   return Err(error)
                       .context("reading DuckDuckGo response after retries");
               }

               wait_before_retry(
                   attempt,
                   &format!("response body error: {error}"),
               )
               .await;

               continue;
           }
       };

       return Ok((status, body));
   }

   unreachable!("retry loop always returns or continues")
}

// ---- request ----------------------------------------------------------------

struct Parsed {
   results: Vec<SearchResult>,
   vqd: Option<String>,
}

async fn fetch(
   cfg: &DuckDuckGoConfig,
   q: &str,
   page: u32,
   vqd: Option<&str>,
) -> Result<Parsed> {
   let query = quote_bangs(q);
   let cookie = cookie_header(cfg);
   let off = offset(page);

   let mut form: Vec<(String, String)> = vec![
       ("q".into(), query),
       ("kl".into(), cfg.kl.clone()),
   ];

   if page <= 1 {
       // First page: obtain the vqd token from the returned HTML form.
       form.push(("b".into(), String::new()));
   } else {
       let token = vqd.context("vqd is required for page 2+")?;

       form.extend([
           ("vqd".into(), token.to_string()),
           ("nextParams".into(), String::new()),
           ("api".into(), "d.js".into()),
           ("o".into(), "json".into()),
           ("v".into(), "l".into()),
           ("dc".into(), (off + 1).to_string()),
           ("s".into(), off.to_string()),
       ]);
   }

   if let Some(df) = cfg.df {
       form.push(("df".into(), df.into()));
   }

   let build = || {
       let mut request = client()
           .post(DDG_URL)
           .header(USER_AGENT, UA)
           .header(ACCEPT, ACCEPT_HEADER)
           .header(ACCEPT_LANGUAGE, &cfg.accept_language)
           .header(REFERER, DDG_URL)
           .header("Sec-Fetch-Dest", "document")
           .header("Sec-Fetch-Mode", "navigate")
           .header("Sec-Fetch-Site", "same-origin")
           .header("Sec-Fetch-User", "?1");

       if let Some(cookie) = &cookie {
           request = request.header(COOKIE, cookie);
       }

       request.form(&form)
   };

   let (status, body) = send_with_retries(build).await?;

   // DDG can use a 303 for a !bang redirect.
   if status == StatusCode::SEE_OTHER {
       return Ok(Parsed {
           results: Vec::new(),
           vqd: None,
       });
   }

   let (parsed, captcha) = parse(&body, off, page_size(page));

   if captcha {
       bail!("DuckDuckGo returned a CAPTCHA challenge");
   }

   Ok(parsed)
}

// ---- parsing ----------------------------------------------------------------

/// Parse the result HTML.
///
/// Returns (parsed results and vqd, is_captcha).
fn parse(
   body: &str,
   rank_offset: u32,
   max_results: usize,
) -> (Parsed, bool) {
   let s = selectors();
   let dom = Html::parse_document(body);

   if dom.select(&s.captcha).next().is_some() {
       return (
           Parsed {
               results: Vec::new(),
               vqd: None,
           },
           true,
       );
   }

   let vqd = dom
       .select(&s.vqd)
       .next()
       .and_then(|input| input.value().attr("value"))
       .map(str::to_string);

   let mut results = Vec::new();

   // Restrict to web-result containers to skip advertisement blocks.
   for item in dom.select(&s.result) {
       if results.len() >= max_results {
           break;
       }

       let Some(a) = item.select(&s.title).next() else {
           continue;
       };

       let Some(href) = a.value().attr("href") else {
           continue;
       };

       let Some(url) = clean_url(href) else {
           continue;
       };

       let title = text(a);

       if title.is_empty() {
           continue;
       }

       let snippet = item
           .select(&s.snippet)
           .next()
           .map(text)
           .unwrap_or_default();

       let rank = rank_offset as usize + results.len();

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

   (
       Parsed { results, vqd },
       false,
   )
}

fn text(el: ElementRef<'_>) -> String {
   el.text()
       .flat_map(str::split_whitespace)
       .collect::<Vec<_>>()
       .join(" ")
}

/// DDG result links can be direct URLs or redirect URLs containing `uddg`.
fn clean_url(href: &str) -> Option<String> {
   let absolute = match href.strip_prefix("//") {
       Some(rest) => format!("https://{rest}"),
       None => href.to_string(),
   };

   let url = Url::parse(&absolute).ok()?;

   if url.host_str().is_some_and(|host| {
       host == "duckduckgo.com"
           || host.ends_with(".duckduckgo.com")
   }) {
       if url.path().starts_with("/l/") {
           let target = url
               .query_pairs()
               .find(|(key, _)| key == "uddg")
               .map(|(_, value)| value.into_owned())?;

           let target_url = Url::parse(&target).ok()?;

           return matches!(target_url.scheme(), "http" | "https")
               .then_some(target);
       }

       return None;
   }

   matches!(url.scheme(), "http" | "https")
       .then_some(absolute)
}

fn favicon(url: &str) -> Option<String> {
   let url = Url::parse(url).ok()?;

   Some(format!(
       "{}://{}/favicon.ico",
       url.scheme(),
       url.host_str()?
   ))
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
   use super::*;

   const FIXTURE: &str = r#"
   <html><body>
     <input type="hidden" name="vqd" value="4-1234567">

     <div id="links" class="results">
       <div class="result results_links result--ad">
         <h2 class="result__title">
           <a href="//duckduckgo.com/y.js?ad=1">Ad</a>
         </h2>
       </div>

       <div class="result results_links results_links_deep web-result">
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

       <div class="result results_links web-result">
         <h2 class="result__title">
           <a class="result__a" href="https://www.rust-lang.org/">
             Rust
           </a>
         </h2>
       </div>

       <div class="result web-result">
         <div>No title link</div>
       </div>
     </div>
   </body></html>
   "#;

   #[test]
   fn parses_results_skips_ads_and_finds_vqd() {
       let (parsed, captcha) = parse(FIXTURE, 0, PAGE1_SIZE);

       assert!(!captcha);
       assert_eq!(parsed.vqd.as_deref(), Some("4-1234567"));
       assert_eq!(parsed.results.len(), 2);

       assert_eq!(parsed.results[0].title, "Example Title");
       assert_eq!(
           parsed.results[0].url,
           "https://example.com/a?x=1"
       );
       assert_eq!(
           parsed.results[0].description,
           "A snippet here"
       );

       assert_eq!(
           parsed.results[1].url,
           "https://www.rust-lang.org/"
       );

       assert!(
           (parsed.results[0].score - 1.0).abs() < 1e-9
       );
       assert!(
           (parsed.results[1].score - 0.5).abs() < 1e-9
       );
   }

   #[test]
   fn page_offsets_are_correct() {
       assert_eq!(offset(1), 0);
       assert_eq!(offset(2), 10);
       assert_eq!(offset(3), 25);
       assert_eq!(offset(4), 40);
   }

   #[test]
   fn page_sizes_are_correct() {
       assert_eq!(page_size(1), 10);
       assert_eq!(page_size(2), 15);
       assert_eq!(page_size(3), 15);
   }

   #[test]
   fn ranks_continue_on_later_pages() {
       let (parsed, _) = parse(FIXTURE, offset(2), NEXT_PAGE_SIZE);

       assert_eq!(parsed.results.len(), 2);

       assert!(
           (parsed.results[0].score - 1.0 / 11.0).abs() < 1e-9
       );

       assert!(
           (parsed.results[1].score - 1.0 / 12.0).abs() < 1e-9
       );
   }

   #[test]
   fn detects_captcha() {
       let html = r#"
           <html><body>
             <form id="challenge-form"></form>
           </body></html>
       "#;

       let (parsed, captcha) = parse(html, 0, PAGE1_SIZE);

       assert!(captcha);
       assert!(parsed.results.is_empty());
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

   #[test]
   fn rejects_non_web_duckduckgo_links() {
       assert_eq!(
           clean_url("//duckduckgo.com/y.js?ad=1"),
           None
       );
   }

   #[test]
   fn quotes_bangs() {
       assert_eq!(
           quote_bangs("rust !w async !"),
           "rust '!w' async !"
       );
   }
}
