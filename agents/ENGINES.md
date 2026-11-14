# Writing search engines for ube

Audience: coding agents (and humans) porting or fixing engines. **Read this whole file before touching code.**
Companions: [`BACKLOG.md`](BACKLOG.md) (what to port next, ranked), [`PROMPT.md`](PROMPT.md) (ready-to-paste prompts), [`tools/sx_engine.py`](tools/sx_engine.py) (read a SearXNG engine fast).

## 0. Rules that override everything

1. **Evidence over memory.** SearXNG's Python is the spec for *what to request*; the live site is the spec for *what comes back*, and the two drift apart (SearXNG's `bing_news` still declares `results: RSS` but parses HTML). If you cannot reach the site, ask the human to run the probes in section 11 and paste the output. **Never guess selectors a second time**: the Bing Videos port took several rounds because of guessed markup, and each round ended only when real HTML was pasted.
2. **Start from a clean tree.** Run `git status` first. If anything is modified that you did not modify, **stop and tell the human**. Do not "keep" or build on code you cannot explain (an unexplained, non-compiling fallback once appeared in a working tree during a bug fix). Run `git status` again before every commit.
3. **Do not claim it works unless you have live evidence.** Say exactly what you verified: "tests pass on fixtures copied from your paste; not run against the live site".
4. **A challenge is a wall, not a puzzle.** Anubis, captchas, DataDome and similar: do not write solvers or spoof your way through. Return `Err` with a clear message and offer the `site:` workaround through an engine that already works. (Decided for the Arch Wiki: every User-Agent got the Anubis page. See `BACKLOG.md`.)
5. **Ask before** adding a dependency, a new `Kind`, or changing `static/search.html`.
6. **No secrets in source.** API keys and tokens come from environment variables. Do not copy keys found in SearXNG.
7. **One engine (or one family) per patch. Zero compiler warnings. Do not reformat or touch unrelated code.**

## 1. What an engine plugs into

- A **source** is `async fn(String, u32) -> anyhow::Result<Vec<SearchResult>>`: `(query, page)`, **page is 1-based**. Register it with `Source::new("name", f)` in the registry file of its kind: `src/sources/{pages,videos,news,forums,images}.rs`. The registry lives in `src/sources/mod.rs`.
- The server (`src/search.rs`) runs **all sources of a kind concurrently, per page**. One timeout covers the *whole* call of a source (`SOURCE_TIMEOUT_MS`, default **8000 ms**): token fetch + search together must fit. Results are cached per `(kind, normalized query, page, source)` for `CACHE_TTL_SECS` (300).
- `Err(..)` shows up as **failed** in the sources pane (with your message as tooltip). `Ok(vec![])` shows as a success with nothing. So **blocked or broken must be `Err`, never a silent empty `Ok`.**
- **Merge:** results with the same normalized URL are merged and their scores summed, after multiplying by the source's weight (`.weight(f64)`, overridable at runtime with `SOURCE_WEIGHTS="name=w,name=w"`). Within a page, results are ordered by score. Consequences: always return the **canonical target URL** (never a tracking/redirect wrapper), and keep scores comparable across engines: `1/(1+rank)`.
- Available crates: `reqwest` (gzip, rustls), `wreq` + `wreq-util` (browser TLS/HTTP2 emulation, used by `google.rs`), `scraper` (CSS selectors), `serde`/`serde_json`, `url`, `moka` (async cache), `tracing`, `anyhow`, `tokio`, `futures-util`, `rand`/`fastrand`. There is **no** date crate and **no** XML parser (see F1 and F7 in `BACKLOG.md`).
- Reference implementations, in the order you should read them: `engines/dockerhub.rs` (JSON API, the smallest), `engines/bing_videos.rs` (HTML scraping with every lesson in this file baked in), `engines/google.rs` (anti-bot client with `wreq`, config struct), `engines/duckduckgo.rs` (token caching across pages, retries).

## 2. Anatomy of an engine file

One file per engine (or family) in `src/sources/engines/`, exported in `engines/mod.rs`. The shape that has worked:

1. Module doc: what it is, endpoint, paging, anything surprising. Say "ported from SearXNG's `<module>.py`".
2. Constants (base URL, `PAGE_SIZE`, selectors as `const &str`).
3. `client()` in a `OnceLock<reqwest::Client>` (or `wreq::Client`).
4. A `Config` struct with a `Default` when region/language/time-range/instance matters (`GoogleConfig`, `BingVideosConfig`), plus two entry points: `x_pages(q, page)` (defaults) and `x_with(cfg, q, page)`.
5. A **pure** `build_url(cfg, q, page)` and a **pure** `parse(body, page) -> Vec<SearchResult>`. No I/O in `parse`: that is what the tests call.
6. Tests with fixtures **copied from real responses**.

### Skeleton: JSON API (compiled and tested; copy it)

```rust
//! Example search, ported from SearXNG's `example.py`.
//!
//! Endpoint: GET https://api.example.org/search?q=&page=&per_page=
//! Paging:   1-based `page`, 10 results per page.
//! Notes:    anything surprising goes here (auth, rate limits, quirks).

use crate::types::SearchResult;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};

const BASE_URL: &str = "https://api.example.org";
/// The engine's REAL page size: scores continue across pages from it.
const PAGE_SIZE: u32 = 10;

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5)) // the server's per-source budget is 8 s in total
            .user_agent("ube/0.1")
            .build()
            .expect("failed to build Example HTTP client")
    })
}

/// Entry point registered in `sources/<kind>.rs`:
/// `Source::new("example", example::example_pages)`.
pub async fn example_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let resp = client()
        .get(format!("{BASE_URL}/search"))
        .query(&[("q", q.as_str()), ("page", &page.to_string()), ("per_page", &PAGE_SIZE.to_string())])
        .header("accept", "application/json")
        .send()
        .await
        .context("Example request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read Example response body")?;
    if status.as_u16() == 429 {
        bail!("Example rate limited the request (HTTP 429)");
    }
    if !status.is_success() {
        let excerpt: String = body.chars().take(300).collect();
        bail!("Example returned HTTP {status}: {excerpt:?}");
    }

    let json: Value = serde_json::from_str(&body).context("Example returned invalid JSON")?;
    let results = parse(&json, page);
    if results.is_empty() && json.get("results").is_none() {
        tracing::warn!("example: no `results` key for {q:?} page {page} (API changed?)");
    }
    Ok(results)
}

/// Pure function, no network I/O: this is what the tests call.
pub fn parse(json: &Value, page: u32) -> Vec<SearchResult> {
    let items = json.get("results").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let mut out = Vec::new();
    for item in items {
        // Skip bad entries instead of failing the page; never unwrap remote data.
        let Some(url) = non_empty(item.get("url")) else { continue };
        let Some(title) = non_empty(item.get("title")) else { continue };

        // First result scores 1.0, second 0.5, ... continuing across pages.
        let rank = (page.max(1) - 1) as usize * PAGE_SIZE as usize + out.len();
        out.push(
            SearchResult::new(title, url, non_empty(item.get("summary")).unwrap_or(""), 1.0 / (1.0 + rank as f64))
                .with("popularity", item.get("stars").and_then(Value::as_u64).unwrap_or(0)),
        );
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

    // Copy a REAL response here (trimmed), not an invented one.
    fn fixture() -> Value {
        json!({ "results": [
            { "title": "A", "url": "https://a.example/1", "summary": "first", "stars": 5 },
            { "title": "", "url": "https://a.example/skipped" },
            { "title": "B", "url": "https://a.example/2" }
        ]})
    }

    #[test]
    fn parses_and_skips_bad_entries() {
        let r = parse(&fixture(), 1);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].title.as_str(), r[0].description.as_str()), ("A", "first"));
        assert_eq!(r[1].description, "");
    }

    #[test]
    fn scores_continue_across_pages() {
        let r = parse(&fixture(), 3);
        assert!((r[0].score - 1.0 / 21.0).abs() < 1e-9); // rank 20
    }

    #[test]
    fn missing_results_key_is_empty() {
        assert!(parse(&json!({}), 1).is_empty());
    }
}
```

### Skeleton: HTML scraper (compiled and tested; copy it)

```rust
//! Example HTML search, ported from SearXNG's `example_html.py`.
//!
//! Endpoint: GET https://www.example.org/search?q=&first=
//! Paging:   `first` = (page-1)*PAGE_SIZE + 1.
//! Notes:    wrapped in <noscript>? page 2+ markup different? write it down here.

use crate::types::SearchResult;

use anyhow::{bail, Context, Result};
use scraper::{ElementRef, Html, Selector};
use std::{sync::OnceLock, time::Duration};
use url::Url;

const BASE_URL: &str = "https://www.example.org";
const PAGE_SIZE: u32 = 10;

// SearXNG XPath -> CSS. Keep them as consts so a layout change is a one-line fix.
const SEL_ITEM: &str = "ol#results > li.result";
const SEL_LINK: &str = "h2 > a";
const SEL_SNIPPET: &str = "p";

struct Selectors {
    item: Selector,
    link: Selector,
    snippet: Selector,
}

fn selectors() -> &'static Selectors {
    static S: OnceLock<Selectors> = OnceLock::new();
    S.get_or_init(|| {
        let p = |s| Selector::parse(s).expect("valid selector");
        Selectors { item: p(SEL_ITEM), link: p(SEL_LINK), snippet: p(SEL_SNIPPET) }
    })
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            // Browser-like only if the site needs it; some sites (Anubis) block browser UAs.
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0")
            .build()
            .expect("failed to build Example HTTP client")
    })
}

pub async fn example_html_pages(q: String, page: u32) -> Result<Vec<SearchResult>> {
    let page = page.max(1);
    let first = (page - 1) * PAGE_SIZE + 1;
    let resp = client()
        .get(format!("{BASE_URL}/search"))
        .query(&[("q", q.as_str()), ("first", &first.to_string())])
        .header("accept-language", "en-US,en;q=0.9") // some sites serve a stripped page without it
        .send()
        .await
        .context("Example request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read Example response body")?;
    if status.as_u16() == 429 {
        bail!("Example rate limited the request (HTTP 429)");
    }
    if !status.is_success() {
        let excerpt: String = body.chars().take(300).collect();
        bail!("Example returned HTTP {status}: {excerpt:?}");
    }

    let results = parse(&body, &Url::parse(BASE_URL)?, page);
    if results.is_empty() {
        // The single most useful line when a scraper breaks: say what the server really sent.
        let title = body.split_once("<title>").and_then(|(_, r)| r.split_once("</title>")).map_or("", |(t, _)| t);
        tracing::warn!(
            "example: 0 results for {q:?} page {page}: HTTP {status}, {} bytes, title {title:?}, \
             marker x{}, start {:?}",
            body.len(),
            body.matches("class=\"result\"").count(),
            body.chars().take(120).collect::<String>(),
        );
    }
    Ok(results)
}

/// Pure function, no network I/O. `base` resolves relative links.
pub fn parse(body: &str, base: &Url, page: u32) -> Vec<SearchResult> {
    let s = selectors();
    // scraper (html5ever) treats <noscript> content as TEXT; lxml (SearXNG) does not.
    // If the real page wraps results in <noscript>, strip the tags first (see bing_videos.rs).
    let dom = Html::parse_document(body);
    let mut out = Vec::new();
    for item in dom.select(&s.item) {
        let Some(link) = item.select(&s.link).next() else { continue };
        let Some(href) = link.value().attr("href") else { continue };
        let Ok(url) = base.join(href) else { continue }; // relative -> absolute
        let title = text(link);
        if title.is_empty() {
            continue;
        }
        let snippet = item.select(&s.snippet).map(text).collect::<Vec<_>>().join(" ");

        let rank = (page.max(1) - 1) as usize * PAGE_SIZE as usize + out.len();
        out.push(SearchResult::new(title, url.as_str(), snippet, 1.0 / (1.0 + rank as f64)));
    }
    out
}

fn text(el: ElementRef<'_>) -> String {
    el.text().flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Copy a REAL page-1 AND a real page-2 response here (trimmed). Page 2+ often differs.
    const FIXTURE: &str = r#"<ol id="results">
        <li class="result"><h2><a href="/a">  Alpha  </a></h2><p>first   snippet</p></li>
        <li class="result"><div>no link</div></li>
        <li class="result"><h2><a href="https://b.example/x">Beta</a></h2></li></ol>"#;

    #[test]
    fn parses_resolves_and_skips() {
        let base = Url::parse(BASE_URL).unwrap();
        let r = parse(FIXTURE, &base, 1);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].url, "https://www.example.org/a");
        assert_eq!((r[0].title.as_str(), r[0].description.as_str()), ("Alpha", "first snippet"));
        assert_eq!(r[1].url, "https://b.example/x");
        assert!((r[1].score - 0.5).abs() < 1e-9);
    }
}
```

## 3. Result fields

`SearchResult::new(title, url, description, score)` then `.icon(url)` and `.with("key", value)` for everything else. `extra` keys are flattened into the JSON, and the page templates bind **by name**:

| kind | template binds | you must set | strongly recommended extras |
|---|---|---|---|
| pages | `icon`, `title`, `url`, `description` | title, url | `icon` = `{scheme}://{host}/favicon.ico` of the result host |
| videos | `thumbnail`, `title`, `duration`, `description` | title, url, `thumbnail` | `duration` as a **string** `M:SS` / `H:MM:SS`; `description` = `"69.2K views - 1 day ago - YouTube - Channel"` (Bing style, joined with ` - `) |
| news | `icon`, `title`, `published`, `description` | title, url | `published` = ISO 8601 UTC string (`2026-11-09T08:00:00Z`) |
| forums | `icon`, `title`, `replies`, `description` | title, url | `replies` as a number; put tags/score/author/answered in `description` |
| images | `thumbnail`, `title` (alt text) | url (landing page), `thumbnail` | `img_src` (full size), `width`, `height` (numbers), `source` (site name) |

Rules: **`title` must be non-empty** (skip the entry otherwise). Thumbnails must be direct image URLs over **https**. Descriptions are **plain text** (strip HTML, normalise whitespace). Skip an image with no thumbnail. Use `published`, not `publishedDate` (`dockerhub.rs` predates this and uses `publishedDate`; fix it if you touch that file).

**SearXNG result dict -> ube**

| SearXNG key | ube |
|---|---|
| `url`, `title` | same |
| `content` | `description` (put `metadata` after it, joined with ` - `) |
| `thumbnail` / `thumbnail_src` | `.with("thumbnail", ..)` |
| `img_src` | `.with("img_src", ..)` |
| `publishedDate` (datetime) | `.with("published", rfc3339)` |
| `length` (timedelta / seconds) | `.with("duration", "H:MM:SS")` |
| `template`, `engine`, `category`, `parsed_url` | drop |
| `author`, `source`, `magnetlink`, `seed`, `leech`, `filesize` ... | extras with the same names |

## 4. Paging and scoring

- `page` is 1-based. Convert to what the site wants (`offset`, `first`, `start`, 0-based `page`...). Write the formula in the module doc.
- **`PAGE_SIZE` is the site's real page size** (Bing videos 35, Bing news 10, Qwant 10 (images 50), StackExchange as requested, Brave: `offset=page-1`). Score `1/(1+rank)` with `rank = (page-1)*PAGE_SIZE + index_in_this_page`, so pages continue the ranking instead of restarting at 1.0.
- A source call fetches **exactly one** page. Do not prefetch.
- **Page 2+ may be a different document.** Bing's continuation pages carry no `vrhm` blob and a different tile layout; you only find that out by capturing page 1 *and* page 2 (section 11).
- Check whether the site really pages before promising it: upstream `bing.py` (web) has no paging at all; `mastodon`, `wikipedia`, `piratebay`, `mwmbl` return one page. Return `Ok(vec![])` for `page > 1` there instead of repeating page 1 (the server dedupes by URL, but you waste a request).

## 5. HTTP

- **reqwest** for JSON APIs and plain HTML. **wreq** (with the profile used in `google.rs`) when the site fingerprints TLS/HTTP2 or header order: Brave, Qwant, Google Scholar, Yandex.
- **User-Agent policy** (differs per site, check which side you are on): Wikimedia, crates.io, arXiv, OpenAlex/Crossref expect an *identifying* UA (with a contact when they ask for one); Unsplash is behind Anubis and blocks common browser UAs but accepts a non-browser one; the Arch Wiki's Anubis rejected every UA; `bing_videos.rs` works with a browser-like (Firefox) UA. Never assume: probe.
- Send `Accept-Language: en-US,en;q=0.9` by default (Bing serves a stripped page without it). Consent cookies when needed (`CONSENT=YES+` for YouTube/Google).
- **Timeouts:** the whole source call has 8 s. Use <= 5 s per request; for two-step flows give the token request ~2-3 s. Never retry in a loop; at most one retry, only for 429/5xx or an expired token, and only if it still fits the budget.
- `reqwest` has `gzip` enabled; do not add manual decompression. Check `status` before parsing the body, and read the body *before* bailing so the error can quote an excerpt.

## 6. Errors and diagnostics

- `bail!` with a message a human can act on: `"Example rate limited the request (HTTP 429)"`, `"Example blocked the request (challenge page)"`. Always `.context(..)` network/JSON errors.
- Detect block pages explicitly (tiny body, `/sorry/`, "captcha", "Making sure you're not a bot", `datadome`) and return `Err`.
- **Zero results is a log event.** When the parser finds nothing, `tracing::warn!` the HTTP status, body length, `<title>`, counts of your key markers, and the first ~120 characters. This one line is what finally cracked the Bing Videos bugs. When an engine is under active debugging, also log ~300 characters around the first marker.
- Never `unwrap()`/index remote data. A bad entry is skipped, not fatal. The only `expect`s allowed are on compile-time constants (selectors, client builder).

## 7. Parsing lessons (all learned the hard way)

**HTML**
- **`<noscript>` is raw text to `scraper`.** `html5ever` does not parse `<noscript>` content as elements (lxml, which SearXNG uses, does). Bing wraps its whole result list in `<noscript>`, so every selector matched nothing. Strip the `<noscript>`/`</noscript>` *tags* before parsing (`strip_noscript` in `bing_videos.rs`; move it to `util.rs` per F1). If curl shows markers but ube parses zero, suspect this first.
- **Find data by content, not by name.** Bing renamed/moved the metadata blob between page 1 (`vrhm` attribute) and page 2+ (`mmeta` attribute only). Scan all attributes for the JSON key you need (`"murl"`), then fall back to scanning the raw text.
- **Several blobs per item are normal; merge them.** Taking the first one lost every title (the container's `mmeta` had only URLs; the child's `vrhm` had the title). Group by item URL, keep the first non-empty value per field.
- **Read fields from the item's own markup when the blob lacks them** (title `div.mc_vtvc_title`, duration `div.mc_bc_rc.items`, views/date/channel in `.mc_vtvc_meta_block` spans). Blob first, markup second, so the layout that already works is untouched.
- **Do not trust one page layout.** Shorts, ads, "related" tiles and page 2+ tiles differ. Parse defensively, test each variant, and accept that some tiles legitimately lack a description.
- **XPath is not CSS.** `contains(@class,"x")` is a *substring* match: `[class*="x"]`. Use the token match `[class~="x"]` unless the site hashes class names (Google's `zMzFAb`). `concat()`/`substring-after()` have no CSS equivalent: extract the node, then post-process in Rust.
- `text()` = all descendant text, whitespace-collapsed (SearXNG `extract_text`). For "direct text nodes only" (XPath `span/text()`), iterate children and keep `Node::Text`.
- Resolve relative links with `Url::join(base, href)`. Parsers decode entities already; do not decode twice.
- **Unwrap redirect links to the real URL:**

| site | wrapper | unwrap |
|---|---|---|
| Bing | `https://www.bing.com/ck/a?...&u=a1<b64>` | query `u`, drop `a1`, pad, base64url-decode |
| Google (WML) | `/url?q=<url>&sa=...` | percent-decode `q` (`unwrap_google_url` in `google.rs`) |
| Yahoo | `.../RU=<url>/RK=2/RS=...` | text after `/RU=`, cut at `/RS` or `/RK`, percent-decode |
| Google Images | `/imgres?imgurl=<full>&imgrefurl=<page>` | read both query params |

**JSON**
- Use `serde_json::Value` + small accessors, not strict structs: a `null` or a number where a string was expected must not drop the whole result (Bing's `du`/`vt`). Accept numbers-as-strings and strings-as-numbers where APIs are sloppy.
- Skip sentinel rows (`No results returned`), redirect rows (`#REDIRECT` in MediaWiki snippets), and entries without URL/title.
- Epoch seconds -> RFC 3339 without a date crate (F1). API snippets often contain HTML (`<span class="searchmatch">`) or entities (`&quot;` in StackExchange titles): convert to text.

## 8. Engine types and their particularities

The `types` column in `BACKLOG.md` uses these names. Most engines are one or two of them.

### T1 `json-api`: documented REST/JSON
*Examples:* wikimedia APIs, StackExchange, GitHub, crates.io, HN (Algolia), Lemmy, Mastodon, OpenAlex, Crossref, arXiv-JSON-ish, Dailymotion, SepiaSearch.
- Implement with `reqwest` + `Value`. Smallest and most stable: do these first.
- Check: identifying User-Agent; **rate limits and quotas** (GitHub anonymous 10/min -> 403/429 = `Err`; StackExchange 300/day and a `backoff` field you must honour; Crossref/OpenAlex "polite pool" via `mailto`; Openverse anonymous limits); `per_page` caps; 0-based vs 1-based page; `offset` vs `page`; response `fields` that must be requested (Dailymotion); gzip.
- Optional auth only through env vars (`GITHUB_TOKEN`).

### T2 `json-internal`: the website's own undocumented JSON
*Examples:* Pinterest, Vimeo, SoundCloud, Pexels, Odysee, Bilibili, Reuters, Semantic Scholar, Unsplash, Qwant, DDG `i.js`/`v.js`/`news.js`.
- No stability promise: schemas drift. Parse tolerantly, keep a **real response as a fixture**, log the diagnostic line, give it a lower weight.
- Usually requires `Referer`/`Origin`/`X-Requested-With` or custom headers (`X-Pinterest-*`, `X-S2-*`) and often a token (T6).

### T3 `html`: static HTML scraping
*Examples:* Bing web/news, Yahoo, PyPI, Bandcamp, pkg.go.dev, torrent sites, regional engines.
- Selectors as `const`s, compiled once. Resolve relative URLs, unwrap redirects (section 7), strip `<noscript>`, normalise whitespace.
- **Consent/cookie/language dependence:** send `Accept-Language`; some sites need a consent cookie.
- Fragile by nature: the zero-results diagnostic (section 6) is mandatory.

### T4 `html+json`: data embedded in the page
*Examples:* YouTube (`ytInitialData`), Flickr, Brave (`kit.start(app, element, {...})`), Bing (`m`/`mmeta`/`vrhm` attributes).
- Locate by marker, then extract with a **string- and escape-aware balanced-bracket scan** (never a greedy regex).
- **Brave's payload is a JavaScript literal, not JSON**: unquoted keys, IIFEs (`(function(a,b){return {...}})(...)`), `void 0`. It needs a small JS-literal-to-JSON converter. Budget it as an L.
- Prefer content-based detection (see section 7) and keep two tiers: structured (attributes/script) then raw scan.

### T5 `xml`: Atom/RSS/OAI/XML APIs
*Examples:* arXiv (Atom), PubMed (eutils XML), bt4g (RSS).
- **ube has no XML parser and `scraper` is wrong for XML** (HTML parsing mangles `<link>`, drops namespaces, lowercases). **Ask the human** before adding `quick-xml` or `roxmltree` (F7).
- arXiv asks for about one request per 3 s: do not burst. PubMed is also T6 (two requests).

### T6 `multi-step` / `token`: a token or second request first
*Examples:* DDG extra (`vqd`), Vimeo (JWT from `vimeo.com/_next/jwt`), SoundCloud (guest `client_id` scraped from the site's JS bundles), Pexels (secret key scraped from bundles), Semantic Scholar (`X-S2-UI-Version`), PubMed (esearch then efetch), Qwant (rotating `datadome` cookie).
- Use one shared token cache (F2): a `moka` cache with `get_with` so concurrent queries do not all fetch the token.
- Budget: token request 2-3 s + search 5 s must fit the 8 s limit; run independent requests with `tokio::join!`.
- On 401/403, invalidate the cached token and retry **once**. Never fetch a token per query.
- If the token source is a scraped JS bundle, the regex will break: log which step failed (`token fetch failed` vs `search failed`).

### T7 `stateful-paging`: page N needs a token from page N-1
*Examples:* YouTube (continuation token), Pinterest (`bookmark`), DDG HTML (`vqd` plus continuation form fields: `duckduckgo.rs` already caches page 1's token for page 2+).
- Sources are stateless `(query, page)` functions, so store the *next-page token* in a cache keyed by `(engine, normalized query, page + 1)` when you serve page N, and read it for page N+1.
- If the token is missing (cache evicted, page 3 asked first), either re-walk the chain within the time budget or return `Err("page 3 requested before page 2")`. **Never invent offsets.**

### T8 `anti-bot`: fingerprinting, captchas, challenges
*Examples:* Google (WML endpoint + Nokia UA + Chrome TLS profile, `google.rs`), Brave, Qwant (DataDome), Yandex/Baidu/Sogou (captcha), Google Scholar, **Anubis** sites (Arch Wiki, Ayo).
- Use `wreq` with an emulation profile; match header order where the site checks it; keep cookies the site hands back.
- Detect the block and return `Err`; do not loop; do not solve captchas or proof-of-work challenges (rule 4).
- **Anubis policy is per site.** Unsplash accepted a non-browser UA; the Arch Wiki returned the challenge (HTTP 200, title `Making sure you're not a bot!`, cookie `techaro.lol-anubis-cookie-verification-*`) for every UA tested. Detect by that title/cookie.
- Weight these low and document that they may be blocked from the user's IP.

### T9 `generic`: config-driven families (one struct, many instances)
*Examples:* MediaWiki (12 instances), StackExchange (3), Lemmy (4), Mastodon (2), Discourse (3), Gitea/Forgejo (2), Hugging Face (3), PeerTube/SepiaSearch, SearXNG's `xpath` (23 instances) and `json_engine` (8).
- One `XConfig` + `x_with(cfg, q, page)`; register each instance as a closure with a name `family-instance` (see the commented `google-fr` example in `pages.rs`).
- For `xpath`/`json_engine`: build the generic engine first (F4/F5) so each of the ~30 instances is a 5-line config. XPath -> CSS translation is manual (section 7).

### T10 `regional` / long tail
*Examples:* Naver, Baidu, Sogou, Quark, Yandex, Seznam, tusksearch, privacywall, vuhuv, zapmeta...
- Useful only for specific languages: verify with **native-language queries**, not "rust". Most are disabled upstream. Do them last, weight 0.3-0.5.

## 9. Particularities by ube kind and by SearXNG category

**pages** (SearXNG `general`/`web`)
- Return the canonical URL (unwrap redirects, drop tracking params). `icon` = host favicon. Snippet = plain text.
- Duplicates across engines are the point: identical URLs merge and gain score, so do **not** invent your own URL shape (no added `www.`, no `http`, no fragment).
- `wikipedia` is a **single-page summary lookup**, not a search: it returns at most one result (title-case the query; 404 is empty).

**IT / packages / repos / code / software wikis** (SearXNG `it`, `packages`, `repos`, `q&a`)
- Today they go to `pages` with a **low weight** (`.weight(0.3)` like Docker Hub), because they would otherwise pollute general results. Put package-style data in extras: `package_name`, `maintainer`, `popularity` (`"1000 pulls, 21000 stars"`), `tags`, `published`.
- `q&a` engines (StackExchange, Discourse) belong on **forums** with `replies`.
- GitHub/StackExchange have anonymous quotas: failing fast with `Err` is correct.
- Ask the human whether to add a dedicated `Kind` before porting more than ~5 of these.

**videos**
- URL = the real video page (not an embed or a redirect). `thumbnail` ~16:9 and >= 300 px wide if the site offers it; fix low-res defaults (Google: `/default.jpg` -> `/hqdefault.jpg`).
- Normalise duration to `H:MM:SS`/`M:SS` (`"37 minutes 39 seconds"`, `2121`, `"PT35M21S"`, `"37:39"` all appear).
- Page 2+ tiles differ (Bing) and short-form tiles have no meta block: a missing description is normal, a missing title is a bug.
- YouTube/Bing wrap everything in `<noscript>`/JSON blobs: section 7.

**news**
- `published` ISO 8601 UTC. Relative text ("2 hours ago", "1d") cannot be converted reliably: compute from `now` only when the unit is unambiguous, otherwise put the text in `description` and omit `published`.
- Put the publisher in `description` (`"Reuters - 2 hours ago"`). Thumbnails are often relative (prefix the site origin).
- The same story appears on many engines under different URLs: canonical URLs matter more here than anywhere.

**forums / social / Q&A**
- `replies` = number of answers/comments. Titles may be HTML-escaped. Link to the thread, not to an API URL.
- One config struct often covers several tabs-worth of instances (StackExchange sites, Lemmy types, Discourse hosts).

**images**
- `thumbnail` is mandatory (the template shows nothing else), `url` is the landing page, `img_src` the full image. Page sizes are large (Bing 35, DDG 100, Qwant 50): set `PAGE_SIZE` accordingly.
- Skip results without a usable thumbnail. Icon sets and stock sites are fine but low priority.

**files / torrents / music / science** (SearXNG `files`, `music`, `science`)
- Same rule as IT: `pages` + low weight, until a kind exists. Extras: `magnetlink`, `seed`, `leech`, `filesize`; `authors`, `doi`, `journal`, `published`; `artist`, `album`.
- Torrent and shadow-library engines are a **legal decision for the maintainer**: ask before porting them. Lyrics engines: link out, never copy lyrics into snippets.

**Not result lists (maps, weather, translate, currency, dictionaries, calculator)**: need a new kind or an "answer card" UI. Stop and ask.

## 10. Reading a SearXNG engine

```
git clone --depth 1 https://github.com/searxng/searxng.git /tmp/searxng
python3 agents/tools/sx_engine.py /tmp/searxng bing_videos      # constants, request(), response(), instances
```
It also prints `SHARES CODE WITH:` for modules that import other engines (`bing_videos` imports `bing` and `bing_images`): read those too.

| SearXNG | ube |
|---|---|
| `request(query, params)` sets `params['url']` | pure `build_url(cfg, q, page)` |
| `params['pageno']` (1-based), `paging`, `max_page`, `page_size` | `page`, `PAGE_SIZE`, doc comment |
| `params['searxng_locale']`, `traits`, `fetch_traits()` | config fields (`market`, `hl`, `lang`, `base_url`) with English defaults. **Do not port traits.** |
| `params['safesearch']`, `params['time_range']` | `Option` config fields like `GoogleConfig.safe` / `time_range`; default off |
| `params['headers']`, `params['cookies']` | `.header(..)`, `Cookie` |
| `params['engine_data']` | T7 token cache |
| `return False` / `params['url'] = None` | `return Ok(vec![])` without a request |
| `resp.json()` / `resp.html()` / `eval_xpath*` / `extract_text` | `serde_json::Value` / `scraper` / CSS / `text()` |
| `raise_for_httperror`, `SearxEngine*Exception` | `bail!` with a clear message |
| `about{}` | the module doc comment. **Ignore `about.results`** (can be stale) |
| `categories` | choose the ube kind (section 9) |
| `enable_http3`, `impersonate` | n/a, or the `wreq` profile |

Read `settings.yml` instances too: they supply `base_url`, `api_site`, `lemmy_type`, `search_type`..., and show which are disabled/inactive (inactive = broken or keyed upstream: do not port).

## 11. Probing the live site

If you have network access, run these yourself. If you do not, **ask the human to run them and paste the full output**. Do not start writing selectors before you have seen real markup for page 1 *and* page 2.

**Probe A: does a plain client get real results, and is page 2 different?**
```sh
UA="Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0"
for p in 1 2 3; do
  URL="https://SITE/search?q=rust&PAGE_PARAM=$p"          # fill in per engine
  curl -sS -A "$UA" -H 'Accept-Language: en-US,en;q=0.9' -D /tmp/p$p.head -o /tmp/p$p.html "$URL"
  echo "page $p: $(head -1 /tmp/p$p.head) | $(wc -c < /tmp/p$p.html) bytes | $(grep -o '<title>[^<]*</title>' /tmp/p$p.html | head -1)"
  for m in MARKER1 MARKER2 noscript; do printf '   %s x%s\n' "$m" "$(grep -o "$m" /tmp/p$p.html | wc -l)"; done
done
```

**Probe B: dump one full item** (this is what finally exposed the Bing continuation layout):
```sh
python3 - <<'EOF'
import re
s = open('/tmp/p2.html').read()
i = s.index('FIRST_ITEM_MARKER')
m = re.search(r'NEXT_ITEM_MARKER', s[i + 20:])
j = i + 20 + m.start() if m else len(s)
print(f"[item is {j-i} chars]"); print(s[i-20:j][:6000])
EOF
```

**Reading the results**
| observation | meaning | action |
|---|---|---|
| HTTP 200, tiny body, title `Making sure you're not a bot!`, `anubis` cookie | Anubis challenge for every client | T8: report, offer `site:` workaround. Do not solve. |
| markers present in curl, **0 parsed in ube** | DOM problem (`<noscript>`!) or the request differs from curl (`Accept-Language`, UA, HTTP/2 vs TLS fingerprint) | strip noscript; copy curl's headers; try `wreq` |
| markers on page 1, absent on page 2+ | continuation pages use a different layout/blob | capture a page-2 item with Probe B; parse by content |
| item found, **some fields empty** | the data is split over several blobs or lives in markup | merge blobs; read from the item's markup |
| 200 + JSON with `error`/`captcha` keys | API-level block | `Err` with the message |
| size drops sharply on later pages | pages are fragments, not full documents | do not require `<html>`/`<title>` |

## 12. Tests

Mandatory, all offline:
1. **Real fixture, byte for byte.** Paste the trimmed response you were given (or fetched) into a `const FIXTURE`. A test that parses an invented fixture proves nothing. Include a **page-2** fixture when the layout can differ.
2. Parsed fields exact: url, title, description, extras (`duration`, `thumbnail`, ...).
3. Scores: first is 1.0, second 0.5, and page 2 starts at `1/(1+PAGE_SIZE)`.
4. `build_url`: every parameter, page offset, optional config (market/time range/instance).
5. Defensive cases: missing fields, `null`/number where a string is expected, entries without URL/title are skipped, empty body/`{}` gives an empty `Vec`.
6. **Regression test per bug, proven to fail without the fix.** Temporarily disable the fix, run the test, confirm it fails with the symptom you were given (e.g. `left: ""`), restore the fix, confirm it passes. Say so in the patch description.

## 13. Registering, naming, weights

- Add `pub mod <engine>;` to `engines/mod.rs` and `Source::new("<name>", <engine>::<engine>_pages)` to the registry of its kind. Lowercase-dash names (`bing-videos`); instance variants `family-instance`.
- Weights: general web engines 1.0; engines that are valid but niche for general queries (packages, science, files, music, Docker Hub) **0.3**; anti-bot/fragile engines 0.5-0.8. Users override with `SOURCE_WEIGHTS`.
- Keep optional variants as **commented examples** in the registry (see `pages.rs`), not as extra enabled sources.
- Every enabled source costs one outbound request per query for its kind. Do not enable a source you are not confident in.

## 14. Workflow and delivery

```sh
git status                                  # must be clean (rule 2)
git checkout -b feat/<engine> <base>
# ... write src/sources/engines/<engine>.rs, register it ...
cargo test                                  # all green, ZERO warnings
git status                                  # only files you meant to change
git add -A && git commit -m "feat(<engine>): <one line>"       # fix(<engine>): ... for bug fixes
git format-patch -1 -o /tmp/out             # N patches: -N; stacked patches keep their order
# verify: fresh checkout of the base, then
git -c user.name=x -c user.email=x@x am /tmp/out/*.patch
```
- Toolchain: `wreq` builds BoringSSL and needs `cmake`, `ninja`, `clang`, `golang`, `pkg-config`; if your `rustc` is older than the crate requires, use `cargo test --ignore-rust-version` and say so.
- One engine/family per patch; fixes to an engine you ported earlier are separate `fix(...)` patches, numbered after it.
- **Final message to the human**, in this order: (1) patch file name and what it stacks on; (2) what it does (endpoint, paging, extras); (3) **differences from SearXNG**; (4) **what is not verified** (live site, page 2, regions); (5) the exact probe command if you need their output; (6) decisions they must make (weight, kind, dependency). Plain prose, no marketing.

## 15. Definition of done

- [ ] `git status` was clean at the start and shows only intended files at the end.
- [ ] Endpoint, paging and quirks traced to the SearXNG source (or live evidence), not memory.
- [ ] Real fixtures (page 1 and, if different, page 2) in the tests; regression tests proven to fail without their fix.
- [ ] `cargo test` green, zero warnings; patch verified with `git am` on a clean base.
- [ ] Blocks and errors are `Err` with a readable message; zero-results diagnostic logged.
- [ ] `title` non-empty, canonical URL, https thumbnails, `published`/`duration`/`replies` formats per section 3.
- [ ] Weight chosen and registered in the right kind; no secrets in code; no new dependency/kind/UI without approval.
- [ ] Final message lists what is **not** verified.
