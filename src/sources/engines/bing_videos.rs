//! Bing Videos search, ported from SearXNG's `bing_videos.py`.
//!
//! Uses Bing's async endpoint (`/videos/asyncv2?async=content`), which
//! returns an HTML fragment. Each video is a `div#mc_vtvc_video*` whose
//! `.vrhdata[vrhm]` attribute holds a JSON blob (`murl` = video URL,
//! `vt` = title, `du` = duration).
//!
//! Result `extra` keys follow the video tab template: `thumbnail`, `duration`.

use crate::types::SearchResult;

use crate::sources::foundations::util::strip_noscript;
use anyhow::{bail, Context, Result};
use scraper::{node::Node, ElementRef, Html, Selector};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, sync::OnceLock, time::Duration};
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

const SEL_INFO: &str = r#"[class~="mc_vtvc_meta_block"] span"#;
const SEL_THUMB: &str = "img[data-src-hq]";
// Continuation pages (first > 1) have no `vrhm` blob and the container's `mmeta` carries
// no title or duration, so those are read from the tile's own markup:
const SEL_TITLE: &str = r#"[class~="mc_vtvc_title"]"#;
const SEL_DURATION: &str = r#"[class~="mc_bc_rc"][class~="items"]"#;
const SEL_THUMB_SRC: &str = r#"[class~="mc_vtvc_th"] img[src]"#;

struct Selectors {
    info: Selector,
    thumb: Selector,
    title: Selector,
    duration: Selector,
    thumb_src: Selector,
}

fn selectors() -> &'static Selectors {
    static S: OnceLock<Selectors> = OnceLock::new();
    S.get_or_init(|| {
        let p = |s| Selector::parse(s).expect("valid selector");
        Selectors {
            info: p(SEL_INFO),
            thumb: p(SEL_THUMB),
            title: p(SEL_TITLE),
            duration: p(SEL_DURATION),
            thumb_src: p(SEL_THUMB_SRC),
        }
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
             title {title:?}, mc_vtvc_video x{}, vrhm x{}; around first murl: {:?}; \
             around first vrhm: {:?}",
            body.len(),
            body.matches("mc_vtvc_video").count(),
            body.matches("vrhm").count(),
            around(&body, "murl", 500),
            around(&body, "vrhm", 200),
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
    /// Thumbnail URL (fallback when the markup has no `img[data-src-hq]`).
    turl: Option<String>,
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
/// Parse an `asyncv2` fragment. Pure function, no network I/O.
///
/// Page 1 carries the metadata in `vrhm` attributes, but continuation pages
/// (`first` > 1) use different markup. So the metadata is found by content,
/// not by name:
/// 1. any attribute whose value is a JSON object containing `"murl"`;
/// 2. failing that, any JSON object containing `"murl"` anywhere in the page
///    (inline script data, attributes with unexpected quoting, ...).
pub fn parse(body: &str, page: u32) -> Vec<SearchResult> {
    let s = selectors();
    let cleaned = strip_noscript(body);
    let dom = Html::parse_document(&cleaned);

    // Tier 1: attributes.
    let mut blobs: Vec<(Meta, Option<ElementRef>)> = Vec::new();
    for el in dom.root_element().descendants().filter_map(ElementRef::wrap) {
        for (_, value) in el.value().attrs() {
            if !value.contains("\"murl\"") {
                continue;
            }
            if let Ok(meta) = serde_json::from_str::<Meta>(value) {
                blobs.push((meta, Some(el)));
            }
        }
    }

    // Tier 2: raw JSON objects anywhere in the page.
    if blobs.is_empty() {
        blobs = scan_json(&cleaned).into_iter().map(|m| (m, None)).collect();
    }

    let mut results = Vec::new();
    for (meta, el) in merge_blobs(blobs) {
        push(&mut results, meta, el, s, page);
    }
    results
}

impl Meta {
    /// Fill in whatever this blob lacks from another blob of the same video.
    fn fill_from(&mut self, other: Meta) {
        if value_text(&self.vt).is_none() {
            self.vt = other.vt;
        }
        if value_text(&self.du).is_none() {
            self.du = other.du;
        }
        if self.turl.as_deref().unwrap_or("").is_empty() {
            self.turl = other.turl;
        }
    }
}

/// Bing puts a blob on several elements of one video (e.g. the container's
/// `mmeta` has the URL but no title; a child's `vrhm` has title and duration).
/// Group blobs by video URL, in first-seen order, keeping the first non-empty
/// value of every field.
fn merge_blobs(blobs: Vec<(Meta, Option<ElementRef<'_>>)>) -> Vec<(Meta, Option<ElementRef<'_>>)> {
    let mut out: Vec<(Meta, Option<ElementRef>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (meta, el) in blobs {
        let Some(url) = meta.murl.clone().filter(|u| u.starts_with("http")) else { continue };
        match index.get(&url) {
            Some(&i) => {
                out[i].0.fill_from(meta);
                out[i].1 = out[i].1.or(el);
            }
            None => {
                index.insert(url, out.len());
                out.push((meta, el));
            }
        }
    }
    out
}

fn push(
    results: &mut Vec<SearchResult>,
    meta: Meta,
    el: Option<ElementRef<'_>>,
    s: &Selectors,
    page: u32,
) {
    let Some(video_url) = meta.murl.clone().filter(|u| u.starts_with("http")) else { return };
    let item = el.map(container);

    // `.//span/text()`: direct text nodes of each span, joined with " - ".
    let info = item
        .iter()
        .flat_map(|item| item.select(&s.info))
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
    // Blob first (page 1: `vrhm`), then the tile's own markup (continuation pages).
    let title = value_text(&meta.vt)
        .or_else(|| {
            let e = item?.select(&s.title).next()?;
            Some(e.value().attr("title").map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
                .unwrap_or_else(|| element_text(e)))
        })
        .filter(|t| !t.is_empty())
        .unwrap_or_default();
    let duration = value_text(&meta.du).or_else(|| {
        let d = element_text(item?.select(&s.duration).next()?);
        (!d.is_empty()).then_some(d)
    });

    let mut r = SearchResult::new(
        title,
        video_url.clone(),
        info,
        1.0 / (1.0 + rank as f64),
    );

    if let Some(d) = duration {
        r = r.with("duration", d);
    }
    let thumb = item
        .and_then(|i| i.select(&s.thumb).next())
        .and_then(|i| i.value().attr("data-src-hq"))
        .map(str::to_string)
        // plain <img src> in the tile (not a lazy-load placeholder / data: URI)
        .or_else(|| {
            item?.select(&s.thumb_src).filter_map(|i| i.value().attr("src")).find(|u| u.starts_with("http")).map(str::to_string)
        })
        .or(meta.turl.filter(|t| t.starts_with("http")))
        .filter(|t| !t.is_empty());
    if let Some(t) = thumb {
        r = r.with("thumbnail", t);
    }
    if let Some(icon) = Url::parse(&video_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{}://{}/favicon.ico", u.scheme(), h)))
    {
        r = r.icon(icon);
    }
    results.push(r);
}

fn element_text(e: ElementRef<'_>) -> String {
    e.text().flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ")
}

/// Find JSON objects containing a `"murl"` key anywhere in `text`
/// (also in HTML-escaped form, `&quot;murl&quot;`).
fn scan_json(text: &str) -> Vec<Meta> {
    let text = text.replace("&quot;", "\"");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find("\"murl\"") {
        let at = from + i;
        // The object's opening brace is before the key; nested objects earlier
        // in the same blob mean it may not be the nearest one, so try a few.
        let found = text[..at]
            .rmatch_indices('{')
            .take(6)
            .find_map(|(start, _)| {
                let mut it = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Meta>();
                match it.next() {
                    Some(Ok(m)) if m.murl.is_some() => Some((m, start + it.byte_offset())),
                    _ => None,
                }
            });
        match found {
            Some((meta, end)) => {
                out.push(meta);
                from = end.max(at + 1);
            }
            None => from = at + 1,
        }
    }
    out
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
use crate::sources::foundations::util::strip_noscript;

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

    // Continuation pages (first > 1) carry no `vrhm`; the blob sits elsewhere.
    #[test]
    fn continuation_page_with_metadata_in_another_attribute() {
        let html = r#"<div class="mc_vtvc" id="mc_vtvc_video_36">
              <a class="x" m='{"murl":"https://a.b/two","vt":"Second page","du":"3:03","turl":"https://t.b/th.jpg"}'></a>
              <div class="mc_vtvc_meta_block"><span>Chan</span></div>
            </div>
            <div><a data-x="{&quot;murl&quot;:&quot;https://a.b/three&quot;,&quot;vt&quot;:&quot;Third&quot;}"></a></div>"#;
        let r = parse(html, 2);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].url, "https://a.b/two");
        assert_eq!(r[0].title, "Second page");
        assert_eq!(r[0].description, "Chan");
        assert_eq!(r[0].extra["duration"], "3:03");
        assert_eq!(r[0].extra["thumbnail"], "https://t.b/th.jpg"); // turl fallback
        assert_eq!(r[1].url, "https://a.b/three");
        assert!((r[0].score - 1.0 / 36.0).abs() < 1e-9); // page 2 ranks continue
    }

    #[test]
    fn falls_back_to_json_embedded_in_script_text() {
        let html = r#"<html><body><script>var d = [
              {"id":1,"meta":{"k":1},"murl":"https://a.b/one","vt":"One","du":"1:11"},
              {"x":{"y":{"z":2}},"vt":"Two","murl":"https://a.b/two","turl":"https://t.b/2.jpg"}
            ];</script><script>var e = "{&quot;murl&quot;:&quot;https://a.b/three&quot;,&quot;vt&quot;:&quot;Three&quot;}";</script>
            </body></html>"#;
        let r = parse(html, 1);
        let urls: Vec<_> = r.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, ["https://a.b/one", "https://a.b/two", "https://a.b/three"]);
        assert_eq!(r[0].title, "One");
        assert_eq!(r[0].extra["duration"], "1:11");
        assert_eq!(r[1].extra["thumbnail"], "https://t.b/2.jpg");
    }

    #[test]
    fn non_video_json_and_non_http_urls_are_ignored() {
        let html = r#"<a x='{"murl":"javascript:alert(1)","vt":"bad"}'></a><a y='{"foo":1}'></a><a z='not json "murl"'></a>"#;
        assert!(parse(html, 1).is_empty());
    }

    // A continuation-page tile, copied verbatim from Bing's response (first=36):
    // `mmeta` only has URLs, so title/duration/thumbnail come from the markup.
    const CONTINUATION_TILE: &str = r##"<div class="dg_u" style="width:304px;height:273px;"><div id="mc_vtvc_video_1" class="mc_vtvc b_canvas emb mc_vtvc_cc  mc_vtvc_fh" mmeta="{&quot;mid&quot;:&quot;A61AF9CD54A4BE7B6259A61AF9CD54A4BE7B6259&quot;,&quot;murl&quot;:&quot;https://www.youtube.com/watch?v=_1njyy87h9A&quot;,&quot;pgurl&quot;:&quot;https://www.youtube.com/watch?v=_1njyy87h9A&quot;,&quot;turl&quot;:&quot;https://ts3.mm.bing.net/th?id=OVF.2KmE%2bctaXHcMeVQewGYVuQ&amp;pid=15.1&amp;W=160&amp;H=120&quot;,&quot;md5&quot;:&quot;f85964b76494fe889602f189b97a2879&quot;}"><a aria-label="I Built An Impossible Secret Exploit Trap In Vanilla Rust... from YouTube · Duration:  37 minutes 39 seconds  · 69.2K views · uploaded on 1 day ago · uploaded by Oblivion · Click to play." data-dc="vtdc_blue" class="mc_vtvc_link" href="/videos/riverview/relatedvideo?&amp;q=rust&amp;&amp;mid=A61AF9CD54A4BE7B6259A61AF9CD54A4BE7B6259&amp;&amp;FORM=VRDGAR" h="ID=video,5013.1"><div class="mc_vtvc_con_rc"
                                 ourl="https://www.youtube.com/watch?v=_1njyy87h9A"><div class="mc_vtvc_th b_canvas"><div class="cico" style="width:302px;height:169px;"><img height="169" width="302" src="https://th.bing.com/th?id=OVF.2KmE%2bctaXHcMeVQewGYVuQ&amp;w=302&amp;h=169&amp;c=7&amp;rs=1&amp;qlt=70&amp;o=7&amp;pid=2.1&amp;rm=3" alt="I Built An Impossible Secret Exploit Trap In Vanilla Rust..." role="" loading="lazy"/></div><div class="mc_vtvc_ban_lo"><div class="vtbc_rc"><div class="mc_bc_rc_w b_smText"><div class="mc_bc_rc items">37:39</div><div class="mc_bc_rc pivot bpi_video_2" data-tag="component.Badge.Pivot"><span title=""><span class="mv_vtvc_play cipg " width="16" height="16"></span></span></div></div></div></div></div><div class="mc_vtvc_meta_w"><div class="mc_vtvc_meta_bg_w"><div class="cico" style="width:302px;height:169px;"><img height="169" width="302" src="https://th.bing.com/th?id=OVF.2KmE%2bctaXHcMeVQewGYVuQ&amp;w=302&amp;h=169&amp;c=7&amp;rs=1&amp;qlt=70&amp;o=7&amp;pid=2.1&amp;rm=3" alt="I Built An Impossible Secret Exploit Trap In Vanilla Rust..." role="" loading="lazy"/></div></div><div class="mc_vtvc_meta"><div class="mc_vtvc_title b_promtxt" title="I Built An Impossible Secret Exploit Trap In Vanilla Rust..."><strong>I Built An Impossible Secret Exploit Trap In Vanilla Rust...</strong></div><div class="mc_vtvc_meta_block_area"><div class="mc_vtvc_meta_block"><div class="mc_vtvc_meta_row"><span class="meta_vc_content">69.2K views</span><span class="meta_pd_content">1 day ago</span></div><div class="mc_vtvc_meta_row"><span>YouTube</span><span class="mc_vtvc_meta_row_channel">Oblivion</span></div></div></div></div></div></div></a></div></div>"##;

    #[test]
    fn parses_continuation_page_tile_from_real_markup() {
        let r = parse(CONTINUATION_TILE, 2);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].url, "https://www.youtube.com/watch?v=_1njyy87h9A");
        assert_eq!(r[0].title, "I Built An Impossible Secret Exploit Trap In Vanilla Rust...");
        assert_eq!(r[0].extra["duration"], "37:39");
        assert_eq!(r[0].description, "69.2K views - 1 day ago - YouTube - Oblivion");
        // the tile's own 302x169 image beats the small `turl` from the blob
        assert_eq!(
            r[0].extra["thumbnail"],
            "https://th.bing.com/th?id=OVF.2KmE%2bctaXHcMeVQewGYVuQ&w=302&h=169&c=7&rs=1&qlt=70&o=7&pid=2.1&rm=3"
        );
    }

    #[test]
    fn continuation_tile_inside_noscript_and_two_tiles() {
        let second = CONTINUATION_TILE
            .replace("mc_vtvc_video_1", "mc_vtvc_video_3")
            .replace("_1njyy87h9A", "SECONDVID1")
            .replace("I Built An Impossible Secret Exploit Trap In Vanilla Rust...", "Second video")
            .replace("37:39", "1:02");
        let html = format!("<html><body><noscript><div class=\"vidres\">{CONTINUATION_TILE}{second}</div></noscript></body></html>");
        let r = parse(&html, 3);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].title.as_str(), r[0].extra["duration"].as_str().unwrap()), ("I Built An Impossible Secret Exploit Trap In Vanilla Rust...", "37:39"));
        assert_eq!((r[1].title.as_str(), r[1].extra["duration"].as_str().unwrap()), ("Second video", "1:02"));
        assert_eq!(r[1].description, "69.2K views - 1 day ago - YouTube - Oblivion");
    }

    #[test]
    fn lazy_placeholder_images_are_not_used_as_thumbnails() {
        let html = r#"<div id="mc_vtvc_video_1" mmeta='{"murl":"https://a.b/v","turl":"https://t.b/ok.jpg"}'>
            <div class="mc_vtvc_th"><img src="data:image/gif;base64,R0lGOD"></div></div>"#;
        assert_eq!(parse(html, 1)[0].extra["thumbnail"], "https://t.b/ok.jpg");
    }

    // Regression: the container's `mmeta` blob (URL only) comes first in document
    // order; title and duration live in the child's `vrhm`. They must be merged.
    #[test]
    fn merges_poorer_container_blob_with_richer_child_blob() {
        let html = r#"<div id="mc_vtvc_video_1" class="mc_vtvc"
              mmeta="{&quot;mid&quot;:&quot;ABC&quot;,&quot;murl&quot;:&quot;https://a.b/v&quot;,&quot;pgurl&quot;:&quot;https://a.b/p&quot;}">
              <div class="vrhdata" vrhm='{"murl":"https://a.b/v","vt":"Real Title","du":"4:20","turl":"https://t.b/x.jpg"}'></div>
              <img data-src-hq="https://img/hq">
              <div class="mc_vtvc_meta_block"><span>1K views</span></div>
            </div>
            <div id="mc_vtvc_video_2" class="mc_vtvc"
              mmeta="{&quot;murl&quot;:&quot;https://a.b/w&quot;}">
              <div class="vrhdata" vrhm='{"murl":"https://a.b/w","vt":"Second","du":99}'></div>
            </div>"#;
        let r = parse(html, 1);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "Real Title");
        assert_eq!(r[0].extra["duration"], "4:20");
        assert_eq!(r[0].description, "1K views");
        assert_eq!(r[0].extra["thumbnail"], "https://img/hq"); // markup beats turl
        assert_eq!(r[1].title, "Second");
        assert_eq!(r[1].extra["duration"], "99");
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
