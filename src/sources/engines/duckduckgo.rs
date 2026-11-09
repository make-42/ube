//! DuckDuckGo web search, ported from SearXNG's `duckduckgo.py` (the no-JS
//! "html" variant: `POST https://html.duckduckgo.com/html/`).
//!
//! Paging works through a `vqd` token ("validation query digest"): page 1 needs
//! none, but its response contains one that every later page of the same query
//! must send. Asking for page 2+ without it is treated as bot behaviour, so the
//! helper never does that. Our server asks for pages concurrently, so page 2+
//! first waits for page 1 (fetched once and shared, see `page1`).
//!
//! DDG blocks IPs it decides are bots. SearXNG's notes say the vqd is derived
//! from query + User-Agent, so the User-Agent here is a fixed constant.
//!
//! If results stop parsing, update the selector constants below.

use crate::types::SearchResult;
use anyhow::{anyhow, bail, Context, Result};
use moka::future::Cache;
use reqwest::header::{ACCEPT_LANGUAGE, COOKIE, REFERER, USER_AGENT};
use scraper::{ElementRef, Html, Selector};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use url::Url;

const DDG_URL: &str = "https://html.duckduckgo.com/html/";
/// Must stay constant: DDG's vqd is tied to it (see module docs).
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

// CSS translations of SearXNG's xpaths.
const SEL_RESULT: &str = r#"#links > div[class*="web-result"]"#; // class "result--ad" blocks don't match
const SEL_TITLE: &str = "h2 a";
const SEL_SNIPPET: &str = r#"a[class*="result__snippet"]"#;
const SEL_VQD: &str = r#"input[name="vqd"]"#;
const SEL_CAPTCHA: &str = "form#challenge-form";

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
        let p = |s| Selector::parse(s).expect("valid selector");
        Selectors {
            result: p(SEL_RESULT),
            title: p(SEL_TITLE),
            snippet: p(SEL_SNIPPET),
            vqd: p(SEL_VQD),
            captcha: p(SEL_CAPTCHA),
        }
    })
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            // a 303 means DDG treated the query as a !bang redirect: don't follow it
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client")
    })
}

#[derive(Clone, Debug)]
pub struct DuckDuckGoConfig {
    /// Region ("kl"), e.g. "wt-wt" (all regions, default), "us-en", "fr-fr", "de-de".
    pub kl: String,
    /// Time filter ("df"): "d" | "w" | "m" | "y"; `None` = any time.
    pub df: Option<&'static str>,
    pub accept_language: String,
}

impl Default for DuckDuckGoConfig {
    fn default() -> Self {
        Self { kl: "wt-wt".into(), df: None, accept_language: "en-US,en;q=0.7".into() }
    }
}

/// Default entry point: `Source::new("duckduckgo", engines::duckduckgo::duckduckgo_pages)`.
pub async fn duckduckgo_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    duckduckgo_with(DuckDuckGoConfig::default(), q, page).await
}

pub async fn duckduckgo_with(cfg: DuckDuckGoConfig, q: String, page: u32) -> Result<Vec<SearchResult>> {
    if q.chars().count() >= 500 {
        bail!("duckduckgo does not accept queries of 500+ characters");
    }
    let page = page.max(1);

    if page == 1 {
        let p1 = page1(&cfg, &q, true).await?;
        if p1.results.is_empty() {
            tracing::warn!("duckduckgo: 0 results parsed for {q:?} (no matches, a !bang redirect, or the layout changed)");
        }
        return Ok(p1.results.clone());
    }

    // Page 2+: needs the vqd from page 1 (waits for page 1 if it's still in flight).
    let p1 = page1(&cfg, &q, false).await?;
    let vqd = p1.vqd.clone().context("duckduckgo gave no vqd token on page 1 (no next page?)")?;
    let parsed = fetch(&cfg, &q, page, Some(&vqd)).await?;
    if let Some(new_vqd) = &parsed.vqd {
        // SearXNG stores the vqd of every response; do the same.
        let updated = Page1 { results: p1.results.clone(), vqd: Some(new_vqd.clone()), at: p1.at };
        page1_cache().insert(cache_key(&cfg, &q), Arc::new(updated)).await;
    }
    Ok(parsed.results)
}

// ---- page-1 + vqd cache ------------------------------------------------------------------

struct Page1 {
    results: Vec<SearchResult>,
    vqd: Option<String>,
    at: Instant,
}

/// vqd stays valid for a while (SearXNG caches it for an hour).
fn page1_cache() -> &'static Cache<String, Arc<Page1>> {
    static C: OnceLock<Cache<String, Arc<Page1>>> = OnceLock::new();
    C.get_or_init(|| Cache::builder().max_capacity(2_000).time_to_live(Duration::from_secs(3600)).build())
}

fn cache_key(cfg: &DuckDuckGoConfig, q: &str) -> String {
    format!("{}|{}|{}", cfg.kl, cfg.df.unwrap_or(""), q)
}

/// Page 1 of `q` plus its vqd. Concurrent callers share one request.
/// `fresh`: page 1 itself wants current results (reuse only if < 30 s old);
/// page 2+ only needs the vqd, so any cached entry will do.
async fn page1(cfg: &DuckDuckGoConfig, q: &str, fresh: bool) -> Result<Arc<Page1>> {
    let key = cache_key(cfg, q);
    let cache = page1_cache();
    if fresh {
        if let Some(p) = cache.get(&key).await {
            if p.at.elapsed() < Duration::from_secs(30) {
                return Ok(p);
            }
            cache.invalidate(&key).await;
        }
    }
    cache
        .try_get_with(key, async {
            let parsed = fetch(cfg, q, 1, None).await?;
            Ok::<_, anyhow::Error>(Arc::new(Page1 { results: parsed.results, vqd: parsed.vqd, at: Instant::now() }))
        })
        .await
        .map_err(|e| anyhow!("{e}"))
}

// ---- request ----------------------------------------------------------------------------

struct Parsed {
    results: Vec<SearchResult>,
    vqd: Option<String>,
}

/// Results of page 2+ start after the first 10, then come in blocks of 15.
fn offset(page: u32) -> u32 {
    if page <= 1 { 0 } else { 10 + (page - 2) * 15 }
}

/// A `!bang` in the query makes DDG redirect away from the results. SearXNG quotes
/// the ones in its bang database; we don't have that database, so we quote all of them.
fn quote_bangs(q: &str) -> String {
    q.split_whitespace()
        .map(|w| if w.len() > 1 && w.starts_with('!') { format!("'{w}'") } else { w.to_string() })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn fetch(cfg: &DuckDuckGoConfig, q: &str, page: u32, vqd: Option<&str>) -> Result<Parsed> {
    let mut form: Vec<(&str, String)> = vec![("q", quote_bangs(q))];
    if page <= 1 {
        form.push(("b", String::new()));
    } else {
        let off = offset(page);
        form.push(("vqd", vqd.context("vqd required for page 2+")?.to_string()));
        form.push(("nextParams", String::new()));
        form.push(("api", "d.js".into()));
        form.push(("o", "json".into()));
        form.push(("v", "l".into()));
        form.push(("dc", (off + 1).to_string()));
        form.push(("s", off.to_string()));
    }
    form.push(("kl", cfg.kl.clone()));
    if let Some(df) = cfg.df {
        form.push(("df", df.into()));
    }

    // DDG also keeps kl/df in cookies; a real browser sends them.
    let mut cookies = Vec::new();
    if cfg.kl != "wt-wt" {
        cookies.push(format!("kl={}", cfg.kl));
    }
    if let Some(df) = cfg.df {
        cookies.push(format!("df={df}"));
    }

    let mut req = client()
        .post(DDG_URL)
        .header(USER_AGENT, UA)
        .header(ACCEPT_LANGUAGE, &cfg.accept_language)
        .header(REFERER, DDG_URL)
        // what a browser submitting the form sends
        .header("Sec-Fetch-Dest", "document")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Site", "same-origin")
        .header("Sec-Fetch-User", "?1")
        .form(&form);
    if !cookies.is_empty() {
        req = req.header(COOKIE, cookies.join("; "));
    }

    let resp = req.send().await.context("duckduckgo request failed")?;
    let status = resp.status();
    if status.as_u16() == 303 {
        return Ok(Parsed { results: vec![], vqd: None }); // bang redirect, nothing to parse
    }
    let body = resp.text().await.context("reading duckduckgo response")?;
    if !status.is_success() {
        bail!("duckduckgo returned HTTP {status}");
    }

    let (parsed, captcha) = parse(&body, offset(page));
    if captcha {
        bail!("duckduckgo is asking for a captcha (IP flagged as a bot)");
    }
    Ok(parsed)
}

// ---- parsing ----------------------------------------------------------------------------

/// Returns (results + next-page vqd, is_captcha). Pure function, no I/O.
fn parse(body: &str, rank_offset: u32) -> (Parsed, bool) {
    let s = selectors();
    let dom = Html::parse_document(body);

    if dom.select(&s.captcha).next().is_some() {
        return (Parsed { results: vec![], vqd: None }, true);
    }

    let vqd = dom.select(&s.vqd).next().and_then(|i| i.value().attr("value")).map(str::to_string);

    let mut results = Vec::new();
    for item in dom.select(&s.result) {
        let Some(a) = item.select(&s.title).next() else { continue };
        let Some(url) = a.value().attr("href").and_then(clean_url) else { continue };
        let title = item.select(&s.title).map(text).collect::<Vec<_>>().join(" ");
        let snippet = item.select(&s.snippet).next().map(text).unwrap_or_default();

        let rank = rank_offset as usize + results.len();
        let mut r = SearchResult::new(title, url.clone(), snippet, 1.0 / (1.0 + rank as f64));
        if let Some(icon) = favicon(&url) {
            r = r.icon(icon);
        }
        results.push(r);
    }
    (Parsed { results, vqd }, false)
}

fn text(el: ElementRef) -> String {
    el.text().flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ")
}

/// DDG's html results link either directly or through `//duckduckgo.com/l/?uddg=<url>`.
fn clean_url(href: &str) -> Option<String> {
    let abs = match href.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => href.to_string(),
    };
    let u = Url::parse(&abs).ok()?;
    if u.host_str() == Some("duckduckgo.com") && u.path().starts_with("/l/") {
        return u.query_pairs().find(|(k, _)| k == "uddg").map(|(_, v)| v.into_owned());
    }
    matches!(u.scheme(), "http" | "https").then_some(abs)
}

fn favicon(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    Some(format!("{}://{}/favicon.ico", u.scheme(), u.host_str()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<html><body>
<div id="links" class="results">
  <div class="result results_links results_links_deep result--ad"><h2 class="result__title"><a href="//duckduckgo.com/y.js?ad=1">Ad</a></h2></div>
  <div class="result results_links results_links_deep web-result"><h2 class="result__title">
    <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fx%3D1&amp;rut=abc">Example   Title</a></h2>
    <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com">A <b>snippet</b> here</a></div>
  <div class="result results_links web-result"><h2><a href="https://www.rust-lang.org/">Rust</a></h2></div>
  <div class="result web-result"><div>no title link</div></div>
</div>
<form action="/html/" method="post"><input type="hidden" name="vqd" value="4-1234567"><input type="submit" value="Next"></form>
</body></html>"#;

    #[test]
    fn parses_results_skips_ads_and_finds_vqd() {
        let (p, captcha) = parse(FIXTURE, 0);
        assert!(!captcha);
        assert_eq!(p.vqd.as_deref(), Some("4-1234567"));
        assert_eq!(p.results.len(), 2);
        assert_eq!(p.results[0].title, "Example Title");
        assert_eq!(p.results[0].url, "https://example.com/a?x=1");
        assert_eq!(p.results[0].description, "A snippet here");
        assert_eq!(p.results[1].url, "https://www.rust-lang.org/");
        assert!((p.results[0].score - 1.0).abs() < 1e-9 && (p.results[1].score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn rank_continues_on_later_pages() {
        let (p, _) = parse(FIXTURE, offset(2));
        assert!((p.results[0].score - 1.0 / 11.0).abs() < 1e-9);
        assert_eq!(offset(1), 0);
        assert_eq!((offset(2), offset(3), offset(4)), (10, 25, 40));
    }

    #[test]
    fn detects_captcha() {
        let (p, captcha) = parse(r#"<html><body><form id="challenge-form"></form></body></html>"#, 0);
        assert!(captcha && p.results.is_empty());
    }

    #[test]
    fn urls_and_bangs() {
        assert_eq!(clean_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.b%2Fc&rut=x").as_deref(), Some("https://a.b/c"));
        assert_eq!(clean_url("https://a.b/").as_deref(), Some("https://a.b/"));
        assert_eq!(clean_url("/relative"), None);
        assert_eq!(quote_bangs("rust  !w async !"), "rust '!w' async !");
    }
}
