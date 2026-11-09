//! Google web search, ported from SearXNG's `google.py`.
//!
//! Uses Google's legacy WML endpoint (`/wml/search`) with a Nokia
//! User-Agent and Chrome 100 Android TLS/HTTP2 emulation.
//!
//! SearXNG uses Chrome 99 Android. wreq-util 0.2.0 does not provide
//! Chrome 99, so Chrome 100 Android is the closest available profile.

use crate::types::SearchResult;

use anyhow::{bail, Context, Result};
use scraper::{ElementRef, Html, Selector};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    time::Duration,
};
use url::Url;
use wreq::Client;
use wreq_util::{Emulation, Platform, Profile};

// ---- Nokia User-Agents -------------------------------------------------------

const NOKIA_USER_AGENTS: &[&str] = &[
    "Nokia7610/2.0 (5.0509.0) SymbianOS/7.0s Series60/2.1 Profile/MIDP-2.0 Configuration/CLDC-1.0",
    "Nokia7610/2.0 (7.0642.0) SymbianOS/7.0s Series60/2.1 Profile/MIDP-2.0 Configuration/CLDC-1.0",
    "Nokia6230/2.0 (05.50) Profile/MIDP-2.0 Configuration/CLDC-1.1",
    "Nokia6230i/2.0 (03.80) Profile/MIDP-2.0 Configuration/CLDC-1.1",
    "Nokia6280/2.0 (03.60) Profile/MIDP-2.0 Configuration/CLDC-1.1",
    "NokiaN72/2.0617.1.0.3 Series60/2.8 Profile/MIDP-2.0 Configuration/CLDC-1.1",
];

fn nokia_user_agent() -> &'static str {
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    let index = NEXT.fetch_add(1, Ordering::Relaxed) % NOKIA_USER_AGENTS.len();

    NOKIA_USER_AGENTS[index]
}

// ---- CSS selectors: translations of SearXNG's XPath selectors ---------------

const SEL_RESULT: &str = r#"div[class*="zMzFAb"]"#;
const SEL_TITLE_LINK: &str = r#"a[class*="fuLhoc"]"#;
const SEL_TITLE: &str = r#"a[class*="fuLhoc"] span[class*="CVA68e"]"#;
const SEL_CONTENT: &str = r#"div[class*="taTFJ"] span[class*="FrIlee"]"#;
const SEL_THUMBNAIL: &str = r#"img[src*="encrypted-tbn"]"#;
const SEL_SUGGESTION: &str = r#"table[class*="HExoMb"] a[class*="ZWRArf"]"#;

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

// ---- HTTP client -------------------------------------------------------------

fn client() -> &'static Client {
    static C: OnceLock<Client> = OnceLock::new();

    C.get_or_init(|| {
        let emulation = Emulation::builder()
            .profile(Profile::Chrome100)
            .platform(Platform::Android)
            .build();

        Client::builder()
            .timeout(Duration::from_secs(15))
            .emulation(emulation)
            .build()
            .expect("failed to build wreq client")
    })
}

// ---- Google configuration ---------------------------------------------------

/// Request parameters (equivalent to SearXNG's `get_google_info`).
#[derive(Clone, Debug)]
pub struct GoogleConfig {
    /// Interface language, e.g. "en", "fr", "zh-TW".
    pub hl: String,

    /// Restrict results to a language, e.g. "lang_en".
    /// Empty means any language.
    pub lr: String,

    /// Restrict results to a country, e.g. "countryFR".
    pub cr: Option<String>,

    /// "off" | "medium" | "high".
    pub safe: Option<&'static str>,

    /// "d" | "w" | "m" | "y" (day/week/month/year).
    pub time_range: Option<&'static str>,

    /// Extra query arguments.
    pub extra: Vec<(String, String)>,
}

impl Default for GoogleConfig {
    fn default() -> Self {
        Self {
            hl: "en".into(),
            lr: "lang_en".into(),
            cr: None,
            safe: None,
            time_range: None,
            extra: vec![],
        }
    }
}

// ---- Public search functions ------------------------------------------------

/// Default entry point:
/// `Source::new("google", google::google_pages)`.
pub async fn google_pages(
    q: String,
    page: u32,
) -> Result<Vec<SearchResult>> {
    google_with(GoogleConfig::default(), q, page).await
}

/// Search Google with custom configuration.
pub async fn google_with(
    cfg: GoogleConfig,
    q: String,
    page: u32,
) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let start = (page - 1) * 10;

    let mut url = Url::parse("https://www.google.com/wml/search")?;

    {
        let mut qp = url.query_pairs_mut();

        qp.append_pair("q", &q);
        qp.append_pair("sca_esv", "1");
        qp.append_pair("hl", &cfg.hl);
        qp.append_pair("lr", &cfg.lr);

        if let Some(cr) = &cfg.cr {
            qp.append_pair("cr", cr);
        }

        qp.append_pair("ie", "utf8");
        qp.append_pair("oe", "utf8");

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

    let request_url = url.to_string();

    // SearXNG pairs a Nokia User-Agent with Chrome Android impersonation.
    // The User-Agent is deliberately overridden here; the TLS and HTTP/2
    // fingerprint still comes from the wreq Chrome 100 Android profile.
    let resp = client()
        .get(url.as_str())
        .header("user-agent", nokia_user_agent())
        .header("accept", "*/*")
        .header("cookie", "CONSENT=YES+")
        .send()
        .await
        .context("Google request failed")?;

    let status = resp.status();

    // Google's CAPTCHA and rate-limit responses.
    if status.as_u16() == 429 || status.as_u16() == 302 {
        bail!("Google blocked the request (CAPTCHA / rate limit)");
    }

    let body = resp
        .text()
        .await
        .context("failed to read Google response body")?;

    if body.len() < 2000 && body.contains("/sorry/") {
        bail!("Google returned a CAPTCHA page");
    }

    if status.as_u16() == 403 {
        let excerpt: String = body.chars().take(500).collect();

        bail!(
            "Google returned HTTP 403 Forbidden. \
             This is an access denial, not a Rust parsing error. \
             Request URL: {request_url}. \
             Response excerpt: {excerpt:?}"
        );
    }

    if !status.is_success() {
        let excerpt: String = body.chars().take(500).collect();

        bail!(
            "Google returned HTTP {status}. \
             Request URL: {request_url}. \
             Response excerpt: {excerpt:?}"
        );
    }

    let (results, _suggestions) = parse(&body, page);

    if results.is_empty() {
        tracing::warn!(
            "google: 0 results parsed for {q:?} page {page} \
             (no matches, or Google's layout changed)"
        );
    }

    Ok(results)
}

// ---- HTML parsing ------------------------------------------------------------

/// Parse a WML results page into (results, suggestions).
///
/// This is a pure function with no network I/O.
pub fn parse(
    body: &str,
    page: u32,
) -> (Vec<SearchResult>, Vec<String>) {
    let s = selectors();

    // Remove the XML declaration before passing the document to the
    // HTML parser.
    let body = if body.trim_start().starts_with("<?xml") {
        body.split_once("?>")
            .map_or(body, |(_, rest)| rest)
    } else {
        body
    };

    let dom = Html::parse_document(body);

    let mut results = Vec::new();

    for item in dom.select(&s.result) {
        let Some(title_el) = item.select(&s.title).next() else {
            continue;
        };

        let title = text(title_el);

        let Some(href) = item
            .select(&s.title_link)
            .next()
            .and_then(|a| a.value().attr("href"))
        else {
            continue;
        };

        let Some(url) = unwrap_google_url(href) else {
            continue;
        };

        let content = item
            .select(&s.content)
            .map(text)
            .collect::<Vec<_>>()
            .join(" ");

        // First result scores 1.0, second 0.5, third 0.333...
        // Scores continue across pages.
        let rank = (page.max(1) - 1) as usize * 10 + results.len();

        let mut result = SearchResult::new(
            title,
            url.clone(),
            content,
            1.0 / (1.0 + rank as f64),
        );

        if let Some(icon) = favicon(&url) {
            result = result.icon(icon);
        }

        if let Some(src) = item
            .select(&s.thumbnail)
            .next()
            .and_then(|i| i.value().attr("src"))
        {
            result = result.with("thumbnail", src);
        }

        results.push(result);
    }

    let suggestions = dom
        .select(&s.suggestion)
        .map(text)
        .filter(|t| !t.is_empty())
        .collect();

    (results, suggestions)
}

// ---- Helpers ----------------------------------------------------------------

fn text(el: ElementRef<'_>) -> String {
    el.text()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Google wraps result links as `/url?q=<real url>&sa=U&...`.
/// Unwrap those links and return the actual destination.
fn unwrap_google_url(raw: &str) -> Option<String> {
    if raw.starts_with("/url?") {
        let url = Url::parse(&format!("https://www.google.com{raw}")).ok()?;

        return url
            .query_pairs()
            .find(|(k, _)| k == "q")
            .map(|(_, v)| v.into_owned());
    }

    raw.starts_with("http")
        .then(|| raw.to_string())
}

/// Construct a favicon URL for a result.
fn favicon(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;

    Some(format!(
        "{}://{}/favicon.ico",
        url.scheme(),
        url.host_str()?
    ))
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<html>
<body>
    <div class="zMzFAb x">
        <a class="fuLhoc"
           href="/url?q=https%3A%2F%2Fexample.com%2Fa%3Fx%3D1&amp;sa=U&amp;ved=abc">
            <span class="CVA68e">Example   Title</span>
        </a>

        <div class="taTFJ">
            <span class="FrIlee">Some <b>snippet</b> text</span>
        </div>

        <img src="https://encrypted-tbn0.gstatic.com/images?q=1">
    </div>

    <div class="zMzFAb">
        <a class="fuLhoc"
           href="/url?q=https://www.rust-lang.org/&amp;sa=U">
            <span class="CVA68e">Rust</span>
        </a>
    </div>

    <div class="zMzFAb">
        <div>block without a title</div>
    </div>

    <table class="HExoMb">
        <tr>
            <td>
                <a class="ZWRArf">rust async</a>
            </td>
        </tr>
    </table>
</body>
</html>"#;

    #[test]
    fn parses_results_and_suggestions() {
        let (results, suggestions) = parse(FIXTURE, 1);

        assert_eq!(results.len(), 2);

        assert_eq!(results[0].title, "Example Title");
        assert_eq!(results[0].url, "https://example.com/a?x=1");
        assert_eq!(results[0].description, "Some snippet text");

        assert_eq!(
            results[0].icon.as_deref(),
            Some("https://example.com/favicon.ico")
        );

        assert!(results[0].extra.contains_key("thumbnail"));

        assert_eq!(
            results[1].url,
            "https://www.rust-lang.org/"
        );

        assert!(
            (results[0].score - 1.0).abs() < 1e-9
                && (results[1].score - 0.5).abs() < 1e-9
        );

        assert_eq!(suggestions, vec!["rust async"]);
    }

    #[test]
    fn scores_continue_across_pages() {
        let (results, _) = parse(FIXTURE, 2);

        assert!(
            (results[0].score - 1.0 / 11.0).abs() < 1e-9
        );
    }

    #[test]
    fn unwraps_urls() {
        assert_eq!(
            unwrap_google_url(
                "/url?q=https%3A%2F%2Fa.b%2Fc&sa=U"
            )
            .as_deref(),
            Some("https://a.b/c")
        );

        assert_eq!(
            unwrap_google_url("https://a.b/").as_deref(),
            Some("https://a.b/")
        );

        assert_eq!(
            unwrap_google_url("/search?q=x"),
            None
        );
    }
}
