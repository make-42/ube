//! Google web search, ported from SearXNG's `google.py`.
//!
//! Like SearXNG, this asks Google's legacy "WML" endpoint (`/wml/search`) for its
//! plain-markup layout while pretending to be a Nokia feature phone. The normal
//! results page needs JavaScript to render, this one doesn't.
//!
//! Google changes the (obfuscated) class names now and then. When results stop
//! parsing, update the selector constants below. SearXNG's `google.py` is the
//! reference for the current values.

use crate::types::SearchResult;
use anyhow::{bail, Context, Result};
use reqwest::header::{ACCEPT, COOKIE, USER_AGENT};
use scraper::{ElementRef, Html, Selector};
use std::{sync::OnceLock, time::Duration};
use url::Url;

// ---- selectors: CSS translations of SearXNG's xpaths ---------------------------------
// `[class*="x"]` is the CSS equivalent of xpath `contains(@class, "x")`.
const SEL_RESULT: &str = r#"div[class*="zMzFAb"]"#;
const SEL_TITLE_LINK: &str = r#"a[class*="fuLhoc"]"#;
const SEL_TITLE: &str = r#"a[class*="fuLhoc"] span[class*="CVA68e"]"#;
const SEL_CONTENT: &str = r#"div[class*="taTFJ"] span[class*="FrIlee"]"#;
const SEL_THUMBNAIL: &str = r#"img[src*="encrypted-tbn"]"#;
const SEL_SUGGESTION: &str = r#"table[class*="HExoMb"] a[class*="ZWRArf"]"#;

// Same list SearXNG rotates through (https://github.com/searxng/searxng/issues/6359).
const NOKIA_USER_AGENTS: [&str; 6] = [
    "Nokia7610/2.0 (5.0509.0) SymbianOS/7.0s Series60/2.1 Profile/MIDP-2.0 Configuration/CLDC-1.0",
    "Nokia7610/2.0 (7.0642.0) SymbianOS/7.0s Series60/2.1 Profile/MIDP-2.0 Configuration/CLDC-1.0",
    "Nokia6230/2.0 (05.50) Profile/MIDP-2.0 Configuration/CLDC-1.1",
    "Nokia6230i/2.0 (03.80) Profile/MIDP-2.0 Configuration/CLDC-1.1",
    "Nokia6280/2.0 (03.60) Profile/MIDP-2.0 Configuration/CLDC-1.1",
    "NokiaN72/2.0617.1.0.3 Series60/2.8 Profile/MIDP-2.0 Configuration/CLDC-1.1",
];

struct Selectors {
    result: Selector,
    title_link: Selector,
    title: Selector,
    content: Selector,
    thumbnail: Selector,
    suggestion: Selector,
}

fn selectors() -> &'static Selectors {
    static S: OnceLock<Selectors> = OnceLock::new();
    S.get_or_init(|| {
        let p = |s| Selector::parse(s).expect("valid selector");
        Selectors {
            result: p(SEL_RESULT),
            title_link: p(SEL_TITLE_LINK),
            title: p(SEL_TITLE),
            content: p(SEL_CONTENT),
            thumbnail: p(SEL_THUMBNAIL),
            suggestion: p(SEL_SUGGESTION),
        }
    })
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client")
    })
}

/// Request parameters (the equivalent of SearXNG's `get_google_info`).
#[derive(Clone, Debug)]
pub struct GoogleConfig {
    /// Interface language, e.g. "en", "fr", "zh-TW".
    pub hl: String,
    /// Restrict results to a language, e.g. "lang_en". Empty = any language.
    pub lr: String,
    /// Restrict results to a country, e.g. "countryFR".
    pub cr: Option<String>,
    /// "off" | "medium" | "high"; `None` leaves Google's default.
    pub safe: Option<&'static str>,
    /// "d" | "w" | "m" | "y" (day/week/month/year); `None` = any time.
    pub time_range: Option<&'static str>,
    /// Extra query args, e.g. for the other Google verticals.
    pub extra: Vec<(String, String)>,
}

impl Default for GoogleConfig {
    fn default() -> Self {
        Self { hl: "en".into(), lr: "lang_en".into(), cr: None, safe: None, time_range: None, extra: vec![] }
    }
}

/// Default entry point: `Source::new("google", google::google_pages)`.
pub async fn google_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    google_with(GoogleConfig::default(), q, page).await
}

pub async fn google_with(cfg: GoogleConfig, q: String, page: u32) -> Result<Vec<SearchResult>> {
    let start = (page.max(1) - 1) * 10;

    let mut url = Url::parse("https://www.google.com/wml/search")?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("q", &q).append_pair("sca_esv", "1").append_pair("hl", &cfg.hl).append_pair("lr", &cfg.lr);
        if let Some(cr) = &cfg.cr {
            qp.append_pair("cr", cr);
        }
        qp.append_pair("ie", "utf8").append_pair("oe", "utf8");
        for (k, v) in &cfg.extra {
            qp.append_pair(k, v);
        }
        if start > 0 {
            qp.append_pair("start", &start.to_string());
        }
        if let Some(t) = cfg.time_range {
            qp.append_pair("tbs", &format!("qdr:{t}"));
        }
        if let Some(s) = cfg.safe {
            qp.append_pair("safe", s);
        }
    }

    // SearXNG also sets `impersonate=chrome99_android` (a spoofed TLS fingerprint).
    // Plain reqwest can't do that; if Google starts blocking you, swap this client
    // for one that can (e.g. the `rquest` crate).
    let ua = NOKIA_USER_AGENTS[fastrand::usize(..NOKIA_USER_AGENTS.len())];
    let resp = client()
        .get(url)
        .header(USER_AGENT, ua)
        .header(ACCEPT, "*/*")
        .header(COOKIE, "CONSENT=YES+") // skips the consent interstitial
        .send()
        .await
        .context("google request failed")?;

    let final_url = resp.url().clone();
    let status = resp.status();
    if final_url.host_str() == Some("sorry.google.com")
        || final_url.path().starts_with("/sorry")
        || status.as_u16() == 429
    {
        bail!("google blocked the request (captcha / rate limit)");
    }
    let body = resp.text().await.context("reading google response")?;
    if body.len() < 2000 && body.contains("/sorry/") {
        bail!("google blocked the request (captcha stub page)");
    }
    if !status.is_success() {
        bail!("google returned HTTP {status}");
    }

    let (results, _suggestions) = parse(&body, page);
    if results.is_empty() {
        tracing::warn!("google: 0 results parsed for {q:?} page {page} (no matches, or the layout changed)");
    }
    // `_suggestions` ("related searches") are parsed but unused; SearchResult has no slot for them yet.
    Ok(results)
}

/// Parse a WML results page into (results, suggestions). Pure function, no I/O.
pub fn parse(body: &str, page: u32) -> (Vec<SearchResult>, Vec<String>) {
    let s = selectors();
    // The page starts with an XML prolog, which an HTML parser doesn't like.
    let body = if body.trim_start().starts_with("<?xml") {
        body.split_once("?>").map_or(body, |(_, rest)| rest)
    } else {
        body
    };
    let dom = Html::parse_document(body);

    let mut results = Vec::new();
    for item in dom.select(&s.result) {
        let Some(title_el) = item.select(&s.title).next() else { continue }; // not a real result block
        let title = text(title_el);
        let Some(href) = item.select(&s.title_link).next().and_then(|a| a.value().attr("href")) else { continue };
        let Some(url) = unwrap_google_url(href) else { continue };
        let content = item.select(&s.content).map(text).collect::<Vec<_>>().join(" ");

        // Plain rank-based score: first hit 1.0, then 1/2, 1/3, ... (continuing across pages).
        let rank = (page.max(1) - 1) as usize * 10 + results.len();
        let mut r = SearchResult::new(title, url.clone(), content, 1.0 / (1.0 + rank as f64));
        if let Some(icon) = favicon(&url) {
            r = r.icon(icon);
        }
        if let Some(src) = item.select(&s.thumbnail).next().and_then(|i| i.value().attr("src")) {
            r = r.with("thumbnail", src);
        }
        results.push(r);
    }

    let suggestions = dom.select(&s.suggestion).map(text).filter(|t| !t.is_empty()).collect();
    (results, suggestions)
}

fn text(el: ElementRef) -> String {
    el.text().flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ")
}

/// Google wraps result links as `/url?q=<real url>&sa=U&...`; unwrap them.
fn unwrap_google_url(raw: &str) -> Option<String> {
    if raw.starts_with("/url?") {
        let u = Url::parse(&format!("https://www.google.com{raw}")).ok()?;
        return u.query_pairs().find(|(k, _)| k == "q").map(|(_, v)| v.into_owned());
    }
    raw.starts_with("http").then(|| raw.to_string())
}

fn favicon(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    Some(format!("{}://{}/favicon.ico", u.scheme(), u.host_str()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?><html><body>
<div class="zMzFAb x"><a class="fuLhoc" href="/url?q=https%3A%2F%2Fexample.com%2Fa%3Fx%3D1&amp;sa=U&amp;ved=abc"><span class="CVA68e">Example   Title</span></a>
  <div class="taTFJ"><span class="FrIlee">Some <b>snippet</b> text</span></div>
  <img src="https://encrypted-tbn0.gstatic.com/images?q=1"></div>
<div class="zMzFAb"><a class="fuLhoc" href="/url?q=https://www.rust-lang.org/&amp;sa=U"><span class="CVA68e">Rust</span></a></div>
<div class="zMzFAb"><div>block without a title</div></div>
<table class="HExoMb"><tr><td><a class="ZWRArf">rust async</a></td></tr></table>
</body></html>"#;

    #[test]
    fn parses_results_and_suggestions() {
        let (r, sug) = parse(FIXTURE, 1);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Example Title");
        assert_eq!(r[0].url, "https://example.com/a?x=1");
        assert_eq!(r[0].description, "Some snippet text");
        assert_eq!(r[0].icon.as_deref(), Some("https://example.com/favicon.ico"));
        assert!(r[0].extra.contains_key("thumbnail"));
        assert_eq!(r[1].url, "https://www.rust-lang.org/");
        assert!((r[0].score - 1.0).abs() < 1e-9 && (r[1].score - 0.5).abs() < 1e-9);
        assert_eq!(sug, vec!["rust async"]);
    }

    #[test]
    fn scores_continue_across_pages() {
        let (r, _) = parse(FIXTURE, 2);
        assert!((r[0].score - 1.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn unwraps_urls() {
        assert_eq!(unwrap_google_url("/url?q=https%3A%2F%2Fa.b%2Fc&sa=U").as_deref(), Some("https://a.b/c"));
        assert_eq!(unwrap_google_url("https://a.b/").as_deref(), Some("https://a.b/"));
        assert_eq!(unwrap_google_url("/search?q=x"), None);
    }
}
