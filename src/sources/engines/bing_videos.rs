//! Bing Videos search, ported from SearXNG's `bing_videos.py`.
//!
//! Uses Bing's async endpoint (`/videos/asyncv2?async=content`), which
//! returns an HTML fragment. Each video is a `div#mc_vtvc_video*` whose
//! `.vrhdata[vrhm]` attribute holds a JSON blob (`murl` = video URL,
//! `vt` = title, `du` = duration).
//!
//! Result `extra` keys follow the video tab template: `thumbnail`, `duration`.

use crate::types::SearchResult;

use anyhow::{bail, Context, Result};
use scraper::{node::Node, ElementRef, Html, Selector};
use serde::Deserialize;
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};
use url::Url;

const BASE_URL: &str = "https://www.bing.com";
/// SearXNG: "to simplify the page count lets use the default of 35 videos per page".
const PAGE_SIZE: u32 = 35;
/// Sent when no market is configured. Bing serves a different (sometimes
/// empty) page to clients without it.
const DEFAULT_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";
const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

// ---- CSS selectors: translations of SearXNG's XPath selectors ----------------

/// Any element carrying the metadata blob, whatever its tag or class.
/// (SearXNG: `//div[contains(@id, "mc_vtvc_video")]` + `.//div[@class="vrhdata"]/@vrhm`;
/// matching on `[vrhm]` alone survives Bing renaming the wrappers.)
const SEL_DATA: &str = "[vrhm]";
const SEL_INFO: &str = r#"[class~="mc_vtvc_meta_block"] span"#;
const SEL_THUMB: &str = "img[data-src-hq]";

struct Selectors {
    data: Selector,
    info: Selector,
    thumb: Selector,
}

fn selectors() -> &'static Selectors {
    static S: OnceLock<Selectors> = OnceLock::new();
    S.get_or_init(|| {
        let p = |s| Selector::parse(s).expect("valid selector");
        Selectors { data: p(SEL_DATA), info: p(SEL_INFO), thumb: p(SEL_THUMB) }
    })
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build Bing HTTP client")
    })
}

// ---- Configuration -----------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct BingVideosConfig {
    /// Bing market, e.g. "en-US", "fr-FR" (`mkt` parameter). `None` = Bing decides.
    pub market: Option<String>,
    /// Interface language, e.g. "en" (`setlang` parameter).
    pub set_lang: Option<String>,
    /// "day" | "week" | "month" | "year".
    pub time_range: Option<&'static str>,
}

/// Bing's `videoage-lt` values, in minutes (SearXNG's `time_map`).
fn time_minutes(range: &str) -> Option<u32> {
    match range {
        "day" => Some(1440),
        "week" => Some(10080),
        "month" => Some(43200),
        "year" => Some(525600),
        _ => None,
    }
}

// ---- Public search functions -------------------------------------------------

/// Default entry point: `Source::new("bing-videos", bing_videos::bing_videos)`.
pub async fn bing_videos(q: String, page: u32) -> Result<Vec<SearchResult>> {
    bing_videos_with(BingVideosConfig::default(), q, page).await
}

pub async fn bing_videos_with(cfg: BingVideosConfig, q: String, page: u32) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let url = build_url(&cfg, &q, page)?;

    let accept_language = cfg.market.as_deref().unwrap_or(DEFAULT_ACCEPT_LANGUAGE);
    let resp = client()
        .get(url.as_str())
        .header("accept", "*/*")
        .header("accept-language", accept_language)
        .send()
        .await
        .context("Bing Videos request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read Bing Videos response body")?;

    if status.as_u16() == 429 {
        bail!("Bing rate limited the request (HTTP 429)");
    }
    if !status.is_success() {
        let excerpt: String = body.chars().take(300).collect();
        bail!("Bing Videos returned HTTP {status}: {excerpt:?}");
    }

    let results = parse(&body, page);
    if results.is_empty() {
        let title = body
            .split_once("<title>")
            .and_then(|(_, r)| r.split_once("</title>"))
            .map_or("", |(t, _)| t);
        tracing::warn!(
            "bing-videos: 0 results parsed for {q:?} page {page}: HTTP {status}, {} bytes, \
             title {title:?}, mc_vtvc_video x{}, vrhm x{}; around first vrhm: {:?}; \
             around first mc_vtvc_video: {:?}",
            body.len(),
            body.matches("mc_vtvc_video").count(),
            body.matches("vrhm").count(),
            around(&body, "vrhm", 400),
            around(&body, "mc_vtvc_video", 300),
        );
    }
    Ok(results)
}

/// Build the request URL (SearXNG's `request()`).
/// Example: https://www.bing.com/videos/asyncv2?q=foo&async=content&first=1&count=35
fn build_url(cfg: &BingVideosConfig, q: &str, page: u32) -> Result<Url> {
    let first = (page.max(1) - 1) * PAGE_SIZE + 1;
    let mut url = Url::parse(&format!("{BASE_URL}/videos/asyncv2"))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("q", q)
            .append_pair("async", "content")
            .append_pair("first", &first.to_string())
            .append_pair("count", &PAGE_SIZE.to_string());
        if let Some(m) = &cfg.market {
            qp.append_pair("mkt", m);
        }
        if let Some(l) = &cfg.set_lang {
            qp.append_pair("setlang", l);
        }
        if let Some(minutes) = cfg.time_range.and_then(time_minutes) {
            // e.g. one week: `&qft= filterui:videoage-lt10080&form=VRFLTR`
            qp.append_pair("form", "VRFLTR")
                .append_pair("qft", &format!(" filterui:videoage-lt{minutes}"));
        }
    }
    Ok(url)
}

// ---- HTML parsing ------------------------------------------------------------

/// Fields are deliberately loose (`Value`): one odd type must not drop a video.
#[derive(Deserialize)]
struct Meta {
    /// Video URL.
    murl: Option<String>,
    /// Title.
    #[serde(default)]
    vt: Value,
    /// Duration, e.g. "4:20" (a number in some responses).
    #[serde(default)]
    du: Value,
}

fn value_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// The video's container: nearest ancestor-or-self whose id contains
/// `mc_vtvc_video`, else the parent of the metadata element.
fn container<'a>(el: ElementRef<'a>) -> ElementRef<'a> {
    let is_video = |e: &ElementRef| e.value().id().is_some_and(|id| id.contains("mc_vtvc_video"));
    if is_video(&el) {
        return el;
    }
    el.ancestors()
        .filter_map(ElementRef::wrap)
        .find(is_video)
        .or_else(|| el.parent().and_then(ElementRef::wrap))
        .unwrap_or(el)
}

/// Bing wraps the whole result list in `<noscript>`. lxml (SearXNG) parses
/// that as normal markup, but html5ever (scraper) treats it as raw text, so
/// the videos would never appear in the DOM. Dropping the wrapper tags makes
/// their contents ordinary elements.
fn strip_noscript(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find('>').map_or(rest.len(), |e| e + 1);
        let tag = &rest[..end];
        let name = tag.trim_start_matches('<').trim_start_matches('/');
        let is_noscript = name.len() >= 8
            && name.is_char_boundary(8)
            && name[..8].eq_ignore_ascii_case("noscript")
            && !name[8..].starts_with(|c: char| c.is_ascii_alphanumeric());
        if !is_noscript {
            out.push_str(tag);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Parse an `asyncv2` fragment. Pure function, no network I/O.
pub fn parse(body: &str, page: u32) -> Vec<SearchResult> {
    let s = selectors();
    let dom = Html::parse_document(&strip_noscript(body));
    let mut results: Vec<SearchResult> = Vec::new();

    for data in dom.select(&s.data) {
        let Some(raw) = data.value().attr("vrhm") else { continue };
        let Ok(meta) = serde_json::from_str::<Meta>(raw) else { continue };
        let Some(video_url) = meta.murl.filter(|u| u.starts_with("http")) else { continue };
        // The blob can appear on more than one element of the same video.
        if results.iter().any(|r| r.url == video_url) {
            continue;
        }
        let item = container(data);

        // `.//span/text()`: direct text nodes of each span, joined with " - ".
        let info = item
            .select(&s.info)
            .flat_map(|span| {
                span.children().filter_map(|c| match c.value() {
                    Node::Text(t) => Some(t.trim().to_string()),
                    _ => None,
                })
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" - ");

        // First result scores 1.0, second 0.5, ... continuing across pages.
        let rank = (page.max(1) - 1) as usize * PAGE_SIZE as usize + results.len();
        let mut r = SearchResult::new(
            value_text(&meta.vt).unwrap_or_default(),
            video_url.clone(),
            info,
            1.0 / (1.0 + rank as f64),
        );

        if let Some(d) = value_text(&meta.du) {
            r = r.with("duration", d);
        }
        if let Some(t) = item
            .select(&s.thumb)
            .next()
            .and_then(|i| i.value().attr("data-src-hq"))
            .filter(|t| !t.is_empty())
        {
            r = r.with("thumbnail", t);
        }
        if let Some(icon) = Url::parse(&video_url).ok().and_then(|u| {
            u.host_str().map(|h| format!("{}://{}/favicon.ico", u.scheme(), h))
        }) {
            r = r.icon(icon);
        }
        results.push(r);
    }
    results
}

/// ~`width` chars of `body` around the first `needle`, for diagnostics.
fn around(body: &str, needle: &str, width: usize) -> String {
    let Some(pos) = body.find(needle) else { return "<absent>".into() };
    let start = body[..pos].char_indices().rev().nth(width / 2).map_or(0, |(i, _)| i);
    body[start..].chars().take(width).collect()
}

// ---- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // `vrhm` is HTML-escaped JSON, exactly as Bing serves it.
    const FIXTURE: &str = r#"
    <div id="mc_vtvc_video_1">
      <div class="vrhdata" vrhm='{"murl":"https://www.youtube.com/watch?v=abc","vt":"Rust in 100 Seconds","du":"2:21"}'></div>
      <img class="rms_img" data-src-hq="https://tse1.mm.bing.net/th?id=1&amp;pid=Api">
      <div class="mc_vtvc_meta_block">
        <span>YouTube</span> <span> Fireship </span> <span><b>nested</b></span>
      </div>
    </div>
    <div id="mc_vtvc_video_2">
      <div class="vrhdata" vrhm='{"murl":"https://vimeo.com/42","vt":"No extras"}'></div>
    </div>
    <div id="mc_vtvc_video_3"><div class="vrhdata" vrhm='{"vt":"no url"}'></div></div>
    <div id="mc_vtvc_video_4"><div class="vrhdata" vrhm='not json'></div></div>
    <div id="mc_vtvc_video_5"><p>no vrhdata</p></div>
    "#;

    #[test]
    fn parses_videos_and_skips_bad_entries() {
        let r = parse(FIXTURE, 1);
        assert_eq!(r.len(), 2);

        assert_eq!(r[0].title, "Rust in 100 Seconds");
        assert_eq!(r[0].url, "https://www.youtube.com/watch?v=abc");
        // direct text nodes only, like XPath span/text(): the nested <b> is not included
        assert_eq!(r[0].description, "YouTube - Fireship");
        assert_eq!(r[0].extra["duration"], "2:21");
        assert_eq!(r[0].extra["thumbnail"], "https://tse1.mm.bing.net/th?id=1&pid=Api");
        assert_eq!(r[0].icon.as_deref(), Some("https://www.youtube.com/favicon.ico"));

        assert_eq!(r[1].url, "https://vimeo.com/42");
        assert_eq!(r[1].description, "");
        assert!(!r[1].extra.contains_key("duration"));
        assert!(!r[1].extra.contains_key("thumbnail"));
    }

    #[test]
    fn parses_videos_wrapped_in_noscript() {
        let html = format!(
            "<html><body><script>var a = 1;</script><noscript>{FIXTURE}</noscript></body></html>"
        );
        let r = parse(&html, 1);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Rust in 100 Seconds");
        assert_eq!(r[0].description, "YouTube - Fireship");

        // attributes and odd casing on the wrapper must not matter
        let html = format!("<NOSCRIPT class=\"x\">{FIXTURE}</NoScript><p>a < b</p>");
        assert_eq!(parse(&html, 1).len(), 2);
    }

    #[test]
    fn strip_noscript_keeps_other_tags() {
        assert_eq!(
            strip_noscript("<p>a</p><noscript><b>x</b></noscript><noscripts>"),
            "<p>a</p><b>x</b><noscripts>"
        );
    }

    #[test]
    fn survives_renamed_wrappers_and_duplicates() {
        let html = r#"<li id="mc_vtvc_video_0" class="new">
              <span class="whatever" vrhm='{"murl":"https://a.b/v","vt":"T","du":"1:00"}'></span>
              <a vrhm='{"murl":"https://a.b/v","vt":"T","du":"1:00"}'></a>
              <img data-src-hq="https://img/1">
              <div class="mc_vtvc_meta_block"><span>Src</span></div>
            </li>"#;
        let r = parse(html, 1);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].description, "Src");
        assert_eq!(r[0].extra["thumbnail"], "https://img/1");
    }

    #[test]
    fn tolerates_odd_metadata_types() {
        let html = r#"<div id="mc_vtvc_video_9"><div class="vrhdata"
            vrhm='{"murl":"https://a.b/v","vt":null,"du":141,"extra":{"x":1}}'></div></div>"#;
        let r = parse(html, 1);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "");
        assert_eq!(r[0].extra["duration"], "141");
    }

    #[test]
    fn scores_continue_across_pages() {
        let r = parse(FIXTURE, 1);
        assert!((r[0].score - 1.0).abs() < 1e-9 && (r[1].score - 0.5).abs() < 1e-9);
        let r = parse(FIXTURE, 2);
        assert!((r[0].score - 1.0 / 36.0).abs() < 1e-9);
    }

    #[test]
    fn builds_url() {
        let u = build_url(&BingVideosConfig::default(), "foo bar", 3).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(u.path(), "/videos/asyncv2");
        assert_eq!(q["q"], "foo bar");
        assert_eq!(q["async"], "content");
        assert_eq!(q["first"], "71"); // (3-1)*35+1
        assert_eq!(q["count"], "35");
        assert!(!q.contains_key("qft"));
    }

    #[test]
    fn builds_url_with_market_and_time_range() {
        let cfg = BingVideosConfig {
            market: Some("fr-FR".into()),
            set_lang: Some("fr".into()),
            time_range: Some("week"),
        };
        let u = build_url(&cfg, "x", 1).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["mkt"], "fr-FR");
        assert_eq!(q["setlang"], "fr");
        assert_eq!(q["form"], "VRFLTR");
        assert_eq!(q["qft"], " filterui:videoage-lt10080");
        assert_eq!(q["first"], "1");
    }
}
