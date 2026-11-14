# Generates agents/BACKLOG.md (run: python3 agents/tools/gen_backlog.py). Edit the table below, then re-run.
# Generates agents/BACKLOG.md. Score = 2*value + ease + reliability + unlock.
# Every endpoint / quirk below was read from searx/engines/*.py (SearXNG a7128d5, 2026-11-14), not from memory;
# anything not confirmed there says "verify".
E = []
def e(id, sx, cat, kind, types, V, Ea, R, U, eff, dflt, notes):
    E.append(dict(id=id, sx=sx, cat=cat, kind=kind, types=types, V=V, E=Ea, R=R, U=U, eff=eff, dflt=dflt, notes=notes))

# ---------------- WEB (pages) ----------------
e("wikipedia","wikipedia","web","pages","json-api",5,5,5,1,"S","on",
  "NOT a search: REST `/api/rest_v1/page/summary/<Title>` returns ONE page. Lowercase queries are title-cased first. 404 and 400 `title-invalid-characters` = empty Ok. Wikimedia policy: send an identifying User-Agent with a contact. Per-language host `<lang>.wikipedia.org`.")
e("bing","bing","web","pages","html, anti-bot(light)",5,3,3,2,"M","off",
  "`ol#b_results > li.b_algo`, link `h2 > a`. Result hrefs are `bing.com/ck/a?...&u=a1<base64url>`: strip `a1`, pad, base64url-decode to get the real URL. Strip `span.algoSlug_icon` from `p` before taking the snippet. Params `q`, `setlang`, `cc` (omit for us/cn/ru), `adlt`. Upstream `bing.py` has NO paging: test `first=` before promising page 2. Bing wraps async responses in `<noscript>` (see bing_videos.rs): reuse the strip helper.")
e("brave","brave (web, images, news, videos)","web","pages","html+json, token(cookies), anti-bot",5,1,3,2,"L","on",
  "One module serves 4 kinds via the URL path (`search|images|news|videos`). Cookies required: `safesearch`, `useLocation=0`, `summarizer=0`, `country`, `ui_lang`. Paging is `offset=page-1`, max 10 pages, time range `tf`. Results are NOT JSON: they sit in a `kit.start(app, element, {node_ids:[..], data:[{type:\"data\",...}]})` JavaScript literal (unquoted keys, IIFEs) that must be bracket-matched and converted to JSON first. Use `wreq` emulation.")
e("wikidata","wikidata","web","pages","json-api, multi-step",3,4,4,0,"M","on","Entity search; see `wikidata.py` for the follow-up lookups it needs for descriptions.")
e("mwmbl","mwmbl","web","pages","json-api",2,5,3,0,"S","off","`GET api.mwmbl.org/api/v1/search/?s=<q>`. Small independent index, no paging.")
e("yahoo","yahoo","web","pages","html, token(cookies)",3,3,2,0,"M","off",
  "Prefs live in cookie `sB` (urlencoded k=v list); a `YBV` cookie from the response is cached and replayed. Paging is odd: `b=page*7+1`, `pz=7`. Links are tracking URLs: take the `http...` after `/RU=` and cut at `/RS`|`/RK` (`parse_url`). Regional domains via a region->domain map. Result blocks `div.algo-sr`.")
e("qwant","qwant (web, images, news, videos)","web","pages","json-internal, anti-bot (DataDome)",3,1,1,2,"L","off",
  "`api.qwant.com/v3/search/{web,images,news,videos}?t&q&count&locale&offset&tgp&device&safesearch`. `count` max 10 (images 50), max 5 pages. A `datadome` cookie rotates on every response: cache it from Set-Cookie and replay. `status != success` -> error_code 24 = rate limit, a body with `url` = captcha. Needs Chrome TLS + header order: `wreq`.")
e("wiby","wiby","web","pages","json-api, generic",2,5,3,1,"S","off","`wiby.me/json/?q=&p=`, keys `URL`/`Title`/`Snippet`. Indie-web index. Port through the generic JSON engine.")
e("encyclosearch / crowdview","encyclosearch, crowdview","web","pages","json-api, generic",2,5,2,1,"S","off","Plain JSON via json_engine config. Small indexes, unstable hosts (crowdview is on onrender.com).")
e("searchmysite","searchmysite","web","pages","html, generic",2,3,2,1,"S","off","xpath config: `div.search-result`, paged with `&page=`.")
e("naver","naver (web, images, news, videos)","web","pages","html+json, regional",2,3,2,2,"M","off","Korean engine; one module, 4 kinds. Only worth it for Korean queries.")
e("sogou / quark / 360search","sogou, sogou images/videos/wechat, quark, 360search (+videos)","web","pages","html/json, regional, anti-bot",2,3,2,1,"M","off","Chinese engines; captcha pages are common (`Captcha` exception paths in code).")
e("baidu","baidu (web, images, kaifa)","web","pages","html+json, anti-bot, regional",2,2,2,1,"M","off","Cookies, captcha handling; Chinese queries only.")
e("yandex","yandex (web, images)","web","pages","html, anti-bot, regional",2,2,1,1,"L","off","Raises captcha frequently; region domains.")
e("seznam","seznam","web","pages","html, regional",2,3,2,0,"S","off","Czech engine.")
e("tusksearch / fireball / vuhuv / privacywall / resulthunter","same names (some with images/news/videos)","web","pages","json/html, long-tail",2,3,2,2,"M","off","Young engines, all disabled upstream; each has its own markup. Low confidence, port last.")
e("xpath long tail","zapmeta, fynd, gabanza, reloado, fastbot, search.ch, wikimini","web","pages","html, generic",1,3,1,1,"S","off","Pure xpath configs upstream. Only after a generic HTML engine exists.")

# ---------------- IMAGES ----------------
e("bing images","bing images","images","images","html+json (attr)",5,3,4,2,"M","on",
  "`GET /images/async?q&mmasync=1&first=(page-1)*35+1&count=35`. Tiles `ul.dgControl_list > li`; the `a.iusc` attribute `m` is a JSON blob: `murl` (full image), `turl` (thumbnail), `purl` (page), `desc`. Title `div.infnmpt a`; format/resolution in `div.imgpt > div > span` split on ` · `. Time range `qft=filterui:age-lt<minutes>`. Same Bing async family as bing_videos.rs: expect `<noscript>` wrapping and DIFFERENT markup from page 2 on. Capture page 1 AND page 2 fixtures.")
e("google images","google images","images","images","html (WML), anti-bot",5,4,3,2,"S","off",
  "Same WML endpoint/client as google.rs with `tbm=isch`. Parse `a[href*=\"/imgres?\"]`: query params `imgurl` (full), `imgrefurl` (page). Thumbnails are `encrypted-tbn` URLs. Extend `GoogleConfig.extra` instead of copying google.rs.")
e("duckduckgo images","duckduckgo images (duckduckgo_extra)","images","images","json-internal, token, stateful-paging",5,2,4,2,"L","on",
  "`duckduckgo.com/i.js?o=json&q&l&vqd&...`. `vqd` must come from the same query AND the same static User-Agent (it is bound to the UA). Needs `Referer: https://duckduckgo.com/` and cookies `ad`/`ah`/`l`/`p`. Paging `s=(page-1)*100`. Queries >= 500 chars are rejected. ube's duckduckgo.rs already caches a vqd between page 1 and 2+; check whether that token works for `i.js` before writing a second fetcher.")
e("wikimedia commons (images/videos/audio/files)","wikicommons.images/.videos/.audio/.files","images","images","json-api",3,5,5,2,"S","on",
  "`commons.wikimedia.org/w/api.php?action=query&generator=search&gsrnamespace=6&gsrsearch=filetype:<bitmap|video|audio|office> <q>&prop=info|imageinfo&iiprop=url|size|mime&iiurlheight=180`. `query.pages` is an object keyed by page id (not ordered): sort deterministically. Strip `File:` and the extension from titles. One module, 4 kinds (video/audio carry `duration`).")
e("openverse","openverse","images","images","json-api",3,5,4,0,"S","off","`api.openverse.org/v1/images/?q&page&page_size=20&format=json`. Anonymous use is rate limited; treat 429 as Err.")
e("unsplash","unsplash","images","images","json-internal",3,4,3,0,"S","on","`unsplash.com/napi/search/photos?query&page&per_page=20`. Anubis blocks common browser User-Agents here: send a non-browser UA (works on this site; Anubis policy is per site, see the Arch Wiki note).")
e("flickr","flickr_noapi","images","images","html+json",3,3,2,0,"M","off","Scrapes the JSON embedded in the search page (no API key). Schema drifts.")
e("pinterest","pinterest","images","images","json-internal, stateful-paging",3,2,3,0,"M","on","`resource/BaseSearchResource/get/?data=<json>` with special `X-Pinterest-*` headers. The next page needs the `bookmark` token from the previous response: cache it per (query, page).")
e("pexels","pexels","images","images","json-internal, token",3,2,2,0,"M","on","API needs a secret key that SearXNG scrapes out of Pexels' JS bundles and caches. Brittle.")
e("giphy / imgur / artstation","giphy, imgur, artstation","images","images","json/html",2,4,2,0,"S","off","Niche; GIF/art sources.")
e("icon sets","iconify, lucide, devicons, material icons, selfhst icons, flaticon, uxwing","images","images","json-api",2,5,4,1,"S","off","Tiny JSON indexes; only useful if you want an icons tab. Results are SVG/PNG, set `thumbnail` to the icon URL.")
e("other stock photos","stocksnap, picjumbo, shopify stock, 1x, 500px, magnific, pixabay (images+videos)","images","images","json/html",2,3,2,1,"M","on/off","Each has its own scrape; pixabay also serves videos.")

# ---------------- VIDEOS ----------------
e("youtube","youtube_noapi","videos","videos","html+json, stateful-paging",5,3,3,1,"M","on",
  "Cookie `CONSENT=YES+`. Page 1: GET `youtube.com/results?search_query&page=` and parse the `ytInitialData` JSON from the HTML. Pages > 1: POST `youtubei/v1/search` with the `continuation` token returned by the previous page (SearXNG keeps it in `engine_data`). ube sources are `(query, page)`: cache the token per (query, page+1) and return Err if it is missing. Time range via `sp=` filter. Durations arrive as `H:MM:SS` strings.")
e("sepiasearch / peertube","sepiasearch, peertube","videos","videos","json-api, generic",4,5,4,1,"S","on",
  "`sepiasearch.org/api/v1/search/videos?search&start=(page-1)*10&count=10&sort=-match&nsfw=`. Peertube instances use the same schema at `<instance>/api/v1/search/videos`: make `base_url` a config field and register one source per instance.")
e("google videos","google videos","videos","videos","html (WML), anti-bot",4,4,3,2,"S","on","`tbm=vid` on the google.rs client. Tiles `div.zMzFAb`, title `span.CVA68e`, link `a.fuLhoc` (unwrap `/url?q=`), thumbnail `img.SygO9d` (rewrite `/default.jpg` to `/hqdefault.jpg`), duration `span.YVIcad`.")
e("duckduckgo videos","duckduckgo videos (duckduckgo_extra)","videos","videos","json-internal, token",4,2,4,2,"L","on","`duckduckgo.com/v.js`; same vqd/UA/cookie rules as DDG images. Build both from one shared helper.")
e("dailymotion","dailymotion","videos","videos","json-api",3,4,4,0,"S","on","`api.dailymotion.com/videos?search&page&limit&fields=<list>`. The API returns only a few fields by default: you must request the `fields` you need (SearXNG keeps a `result_fields` list). `family_filter`, `languages`, `thumbnail_ratio=original`, `password_protected=false`.")
e("odysee","odysee","videos","videos","json-internal",3,4,3,0,"S","off","`lighthouse.odysee.tv/search?s&size=20&from&include=channel,thumbnail_url,title,description,duration,release_time&mediaType=video`; result URL is `https://odysee.com/<name>:<claimId>` (built from the hit, not returned); thumbnails go through `thumbnails.odycdn.com/optimize/...`.")
e("vimeo","vimeo","videos","videos","json-internal, token",3,3,3,0,"M","on","Needs a JWT from `vimeo.com/_next/jwt` (header `x-requested-with: XMLHttpRequest`), cached. Then `api.vimeo.com/search?filter_type=clip` with `Authorization: jwt <token>` and `Accept: application/vnd.vimeo.*+json;version=3.3`.")
e("german public media","media.ccc.de, mediathekviewweb","videos","videos","json-api",2,5,3,0,"S","off","`api.media.ccc.de`, `mediathekviewweb.de/api/query`. Niche but trivial.")
e("bilibili","bilibili","videos","videos","json-internal, token, regional",3,2,2,0,"M","off","`api.bilibili.com/x/web-interface/search/type`; sends a random `buvid3` cookie and `Referer: https://www.bilibili.com/`.")
e("rumble / bitchute / niconico / iqiyi / acfun / ina","same names","videos","videos","html/json, long-tail",2,3,2,0,"M","off","Per-site scrapes; bitchute uses `api.bitchute.com/api/beta/search/videos` (verify auth needs).")

# ---------------- NEWS ----------------
e("google news","google_news","news","news","html (WML), anti-bot",4,4,3,2,"S","on","`tbm=nws`. Links `a[href*=\"/url?q=\"]` (unwrap, dedupe, skip `google.com/search`). Spans: `M3vVJe`/`fuLhoc` title, `dXDvrc` source, `YVIcad` date (RELATIVE text like \"2 hours ago\").")
e("bing news","bing_news","news","news","html, anti-bot(light)",4,3,3,2,"M","on","Despite the module's `about` saying RSS, the code parses HTML from `/news/infinitescrollajax?q&InfiniteScroll=1&first=(page-1)*10+1&SFX=(page-1)&form=PTFTNR`: `div.newsitem`, `a.title`, `.snippet`, `.source span[aria-label]`. Thumbnails are relative (prefix `https://www.bing.com/`). Time range via `qft`. Dates are relative text.")
e("duckduckgo news","duckduckgo news (duckduckgo_extra)","news","news","json-internal, token",4,2,4,2,"L","on","`duckduckgo.com/news.js`; results carry an epoch `date`: convert to ISO 8601 for `published`.")
e("reuters","reuters","news","news","json-internal, anti-bot",3,3,2,0,"M","on","Internal content API; breaks often.")
e("regional news","tagesschau, ansa, il post, abcnyheter, sogou wechat","news","news","json/html, regional",2,4,2,0,"S","off","tagesschau uses the documented `/api2u/search` JSON API (bund.dev); the rest are scrapes.")

# ---------------- FORUMS / SOCIAL / Q&A ----------------
e("stackexchange (stackoverflow, askubuntu, superuser, any site)","stackexchange x3","q&a","forums","json-api, generic",5,5,5,2,"S","on",
  "`api.stackexchange.com/2.3/search/advanced?q&page&pagesize&site=<api_site>&sort&order=desc`. Responses are gzip (reqwest has `gzip`). Anonymous quota is 300 requests/day/IP and a `backoff` field must be honoured: return Err, never hammer. Titles are HTML-escaped. URL is `https://<site>.com/q/<question_id>`. Map `answer_count` to `replies`. One config struct, many sources.")
e("hackernews","hackernews","it","forums","json-api",4,5,5,0,"S","off","`hn.algolia.com/api/v1/search`, page is 0-based, `tags=story`. Empty query returns the front page upstream (ube never sends empty queries). `created_at_i` is epoch seconds, URL `news.ycombinator.com/item?id=<objectID>`, `num_comments` to `replies`.")
e("lemmy","lemmy posts/comments/communities/users","social","forums","json-api, generic",3,5,4,2,"S","on","`<instance>/api/v3/search?q&page&type_=Posts|Comments|Communities|Users`. Default instance lemmy.ml; make `base_url` + type config fields.")
e("mastodon / tootfinder","mastodon users/hashtags, tootfinder","social","forums","json-api",2,5,4,1,"S","on","`/api/v2/search?q&type=accounts|hashtags&resolve=false&limit=40`. No paging.")
e("discourse communities","discourse (python, caddy, pi-hole)","q&a","forums","json-api, generic",2,4,3,1,"S","off","Per-instance `base_url`, `search.json`. One config struct.")
e("boardreader","boardreader","social","forums","json-internal",3,3,2,0,"M","off","Forum aggregator; the most forum-like source in the list.")
e("lobste.rs / habr / 9gag","lobste.rs, habrahabr, 9gag","social","forums","html, generic",2,3,3,0,"S","off","lobste.rs and habr are xpath configs; 9gag has a JSON endpoint.")

# ---------------- IT / CODE / PACKAGES (target kind: pages, low weight) ----------------
e("github","github","it","pages","json-api",5,5,4,0,"S","on","`api.github.com/search/repositories?sort=stars&order=desc&q`, header `Accept: application/vnd.github.preview.text-match+json`. Anonymous limit is 10 requests/minute: treat 403/429 as Err. Read an optional `GITHUB_TOKEN` env var; never hardcode one. (\"github code\" is inactive upstream because it needs a token.)")
e("mdn","mdn (json_engine)","it","pages","json-api, generic",5,5,5,1,"S","on","`developer.mozilla.org/api/v1/search?q&page`: `documents[].mdn_url/title/summary`. URLs are relative to `developer.mozilla.org`.")
e("crates.io","crates","it","pages","json-api",4,5,5,0,"S","off","`crates.io/api/v1/crates?q&page&per_page=10`. crates.io's data-access policy requires an identifying User-Agent.")
e("gitea / forgejo","codeberg, gitea.com","it","pages","json-api, generic",3,5,4,2,"S","off","Same API on every instance: `base_url` config, one source per instance.")
e("huggingface","huggingface models/datasets/spaces","it","pages","json-api, generic",3,5,4,2,"S","off","`huggingface.co/api/{models,datasets,spaces}?search=<q>&direction=-1`. NO paging upstream (return `Ok(vec![])` for page > 1). One config field selects the type.")
e("mankier","mankier (json_engine)","it","pages","json-api, generic",3,5,4,1,"S","on","`mankier.com/api/v2/mans/?q=`.")
e("npm","npm","it","pages","json-api",4,5,3,0,"S","off","SearXNG uses the npms.io API (`api.npms.io/v2/search?q&from&size=25`), NOT registry.npmjs.org. Check npms.io is still alive before porting; the registry's `/-/v1/search` is the alternative (verify).")
e("microsoft learn","microsoft_learn","it","pages","json-api",3,5,4,0,"S","off","`learn.microsoft.com/api/search?search=<q>&$top=10&$skip=(page-1)*10` (note the `$` prefixes).")
e("hex / packagist / metacpan","hex, packagist (json_engine), metacpan","it","pages","json-api",3,5,4,1,"S","off","hex: `hex.pm/api/packages/?search=&page=&per_page=&sort=`; packagist: `packagist.org/search.json?q=&page=`; metacpan: a POST with an Elasticsearch-style JSON body to `fastapi.metacpan.org/v1/file/_search` (read metacpan.py for the body).")
e("gitlab","gitlab","it","pages","json-api",3,4,4,0,"S","off","Per-instance `base_url` (gitlab.com default, gitlab.gnome.org example).")
e("pypi","pypi","it","pages","html",4,3,3,0,"S","on","PyPI has no search API: scrape the HTML search page.")
e("nvd (CVEs)","nvd","it","pages","json-internal",3,4,3,0,"S","off","`nvd.nist.gov/extensions/nudp/services/json/nvd/cve/search/results`.")
e("rubygems / pub.dev / hoogle / pkg.go.dev / lib.rs / anaconda","same names","it","pages","html, generic",3,3,3,1,"S","off","Scrapes; rubygems, pub.dev, hoogle, anaconda are xpath configs.")
e("distro packages","alpine, void, cachy os, repology(inactive)","it","pages","html/json",2,4,3,1,"S","off","Small per-distro APIs.")
e("app stores","fdroid, apple app store, google play, apkmirror","it","pages","html/json",2,3,2,0,"S","off","apple app store is a JSON API; the others are scrapes.")
e("sourcehut / bitbucket / ollama","same names","it","pages","html",2,3,2,0,"S","off","Scrapes.")
e("arch wiki (BLOCKED)","archlinux","it","pages","html, anti-bot (Anubis)",3,1,1,0,"-","on",
  "BLOCKED: tried 2026-11-10, wiki.archlinux.org serves the Anubis challenge page (HTTP 200, `Making sure you're not a bot!`) to every User-Agent tested. Workaround: query a general engine with `site:wiki.archlinux.org`. Do not retry without new evidence.")

# ---------------- SCIENCE (target kind: pages, low weight) ----------------
e("openalex","openalex","science","pages","json-api",3,5,5,0,"S","off","`api.openalex.org/works?search&page&per-page=10&sort=relevance_score:desc`. Add a `mailto` to join the polite pool.")
e("crossref","crossref","science","pages","json-api",3,5,5,0,"S","off","`api.crossref.org/works`. Polite pool via `mailto`.")
e("europepmc","europepmc","science","pages","json-api",3,5,5,0,"S","on","`ebi.ac.uk/europepmc/webservices/rest/search`.")
e("arxiv","arxiv","science","pages","xml",4,3,5,0,"M","on","`export.arxiv.org/api/query?search_query=all:<q>&start&max_results` returns Atom XML. ube has no XML parser: ask before adding `quick-xml`/`roxmltree`. Be polite, arXiv asks for about one request per 3 s.")
e("semantic scholar","semantic_scholar","science","pages","json-internal, token",4,2,3,0,"M","on","POST JSON to `semanticscholar.org/api/1/search` with headers `X-S2-UI-Version` (scraped) and `X-S2-Client`.")
e("pubmed","pubmed","science","pages","xml, multi-step",3,2,5,0,"M","on","Two requests: eutils `esearch` (XML list of PMIDs), then `efetch` with those IDs for the details. Both must fit in the 8 s source timeout: fetch with a tight per-request timeout.")
e("google scholar","google_scholar","science","pages","html, anti-bot",4,2,2,1,"M","on","Captcha-prone; reuse the google.rs client and cookies.")
e("other science","pdbe, openaire (publications, datasets), astrophysics data system(inactive)","science","pages","json",1,4,3,0,"S","on","Niche.")

# ---------------- FILES / MUSIC (target kind: pages, low weight) ----------------
e("piratebay","piratebay","files","pages","json-api",3,5,4,0,"S","on","`apibay.org/q.php?q=&cat=`. A no-hit response is a single row named `No results returned`: filter it. Build the magnet link from `info_hash` + a tracker list. Legal note: decide with the maintainer before shipping torrent sources.")
e("torrent sites","nyaa, solidtorrents, bt4g (RSS/XML), kickass, 1337x, btdigg, tokyotoshokan, findfiles","files","pages","html",2,3,2,0,"M","on/off","Mirror domains change often: make the host configurable.")
e("book libraries","annas archive, library genesis","files","pages","html, anti-bot",3,2,2,0,"M","off","Legal and mirror-stability concerns, see above.")
e("deezer","deezer","music","pages","json-api",3,5,4,0,"S","off","`api.deezer.com/search?q=&index=<offset>`.")
e("mixcloud","mixcloud","music","pages","json-api",2,5,4,0,"S","on","`api.mixcloud.com/search/?q=&type=cloudcast&limit=10&offset=`.")
e("genius","genius","music","pages","json-api",3,4,4,0,"S","on","`genius.com/api/search/multi?q=&page=&per_page=5`. Link out only; never copy lyrics into snippets.")
e("soundcloud","soundcloud","music","pages","json-internal, token",4,2,3,0,"M","on","Needs a guest `client_id` scraped from soundcloud.com's `/assets/*.js` bundles (regex), cached; refetch once on 401. Then `api-v2.soundcloud.com/search?q&offset&limit=10&client_id=`.")
e("bandcamp","bandcamp","music","pages","html",3,3,3,0,"S","on","HTML scrape; embed URLs built from result ids.")
e("radio browser / yandex music","radio_browser, yandex_music","music","pages","json",2,3,3,0,"M","on","radio_browser needs a startup `init()` to pick a server.")

# ---------------- MEDIAWIKI FAMILY ----------------
e("mediawiki family","wikibooks, wikiquote, wikisource, wikiversity, wikivoyage, wikispecies, wikinews, wiktionary, gentoo wiki, nixos wiki, minecraft wiki, free software directory","web","pages","json-api, generic",3,5,5,2,"S","on/off",
  "One engine, many wikis. `<base>/w/api.php?action=query&list=search&format=json&srsearch&sroffset=(page-1)*5&srlimit=5&srwhat=text|title`. `{language}` in `base_url` is substituted. Skip rows whose snippet starts with `#REDIRECT`. URL is `<base>/wiki/<Title with _>`, plus `#<sectiontitle>` when present. Snippets contain `<span class=\"searchmatch\">` HTML: convert to text. wikinews belongs to the news tab. (arch wiki is NOT part of this family for ube: Anubis.)")

# ---------------- needs a new kind / UI feature ----------------
N = []
def n(name, sx, feat, V, Ea, R, notes): N.append(dict(name=name, sx=sx, feat=feat, V=V, E=Ea, R=R, notes=notes))
n("openstreetmap / photon / apple maps","openstreetmap, photon, apple_maps","Kind::Maps + a map widget",4,4,4,"Nominatim/Photon geocoding JSON is easy; the work is the UI.")
n("weather","wttr.in, openmeteo, duckduckgo weather","answer card",3,5,4,"Single answer, not a result list.")
n("translate / dictionaries","lingva, dictzone, mymemory, mozhi, wordnik, jisho, duden, etymonline","answer card / infobox",3,4,3,"Answer-style results.")
n("currency / calculator","currency_convert, wolframalpha_noapi, duckduckgo definitions","answer card",3,3,3,"Answer-style results; wolframalpha_noapi is fragile.")
n("movies / books / media","imdb, tmdb, rottentomatoes, moviepilot, senscritique, openlibrary, goodreads, steam, podchaser, fyyd","Kind::Media",2,3,3,"Mostly HTML scrapes with rich metadata.")
n("shopping / misc","geizhals, chefkoch, destatis, bpb, emojipedia","-",1,3,2,"Regional or niche.")
n("tineye (reverse image)","tineye","upload UI",2,2,2,"Needs image upload.")

SKIP = """**Inactive upstream (SearXNG itself ships them disabled as broken/blocked/keyed):** startpage (+images/news), mojeek (+images/news), swisscows (+images/news/videos), marginalia, dogpile (all), luxxle (all), findborg (all), tiger, tonline, xprivo, searchrockit, searchzee, seekninja, s1search, kukei, keenable, jina, exaapi, braveapi, azure, cloudflareai, deepl, libretranslate, ebay, deviantart, pixiv, wallhaven, openclipart, piped(+music), z-library, github code, springer nature, core.ac.uk, astrophysics data system, freesound, flickr_api, youtube_api, yandex api, wolframalpha_api, yahoo news, sina, ayo (behind Anubis), grokipedia, neocities, and the demo/self-hosted ones (elasticsearch, kavunka, searchrockit...).
**Need an API key upstream:** every `*_api` engine above. Do not port keyed engines unless the human supplies a key via an env var; never hardcode keys (the Pexels key in SearXNG is a scraped site key, not a secret you should copy).
**Self-hosted / not a public service:** yacy, searxng instances, elasticsearch, meilisearch-style engines.
**Onion services:** ahmia, torch (need Tor routing).
**Blocked here:** Arch Wiki (see its row)."""

def score(x): return 2*x['V']+x['E']+x['R']+x['U']
for x in E: x['S']=score(x)
E.sort(key=lambda x:(-x['S'],-x['E'],-x['V'],x['id']))
def tier(s):
    return "P0" if s>=18 else "P1" if s>=16 else "P2" if s>=13 else "P3" if s>=10 else "P4"
import collections
cnt=collections.Counter(tier(x['S']) for x in E)
out=[]
w=out.append
w("# Engine backlog: SearXNG engines to port to ube\n")
w("Generated from SearXNG commit `a7128d5` (2026-11-14): `searx/engines/*.py` (253 modules) and `searx/settings.yml` (353 configured instances, 230 disabled and 91 inactive by default). Endpoints and quirks below were read from that code, not from memory. Where something could not be confirmed it says **verify**.\n")
w("Read `agents/ENGINES.md` first: it explains the conventions, the engine *types*, and the traps. Pick the next engine from the top of the table that is not `done`.\n")
w("## How the priority is computed\n")
w("`score = 2*value + ease + reliability + unlock`, each 1 to 5 (unlock 0 to 2). Tiers: **P0** >= 18, **P1** 16-17, **P2** 13-15, **P3** 10-12, **P4** < 10.\n")
w("- **value**: how useful the results are to a private metasearch (quality, uniqueness, coverage of the tab).\n- **ease**: 5 = one documented JSON GET, 3 = HTML scrape or a small two-step, 1 = anti-bot / TLS fingerprinting / JS-literal parsing.\n- **reliability**: how stable the upstream is (documented API and enabled-by-default upstream score high; captcha/token dances score low).\n- **unlock**: tabs or instances that come almost free once this exists (Google `tbm` variants, Bing family, config-driven families).\n- **effort**: S = under ~150 lines, M = a day, L = several iterations and live debugging.\n")
w("Already ported (do not redo): `google` (pages), `duckduckgo` (pages, HTML endpoint), `docker hub` (pages, weight 0.3), `bing videos` (videos).\n")
w("## Phase 0: shared foundations (do these before the long tail)\n")
w("Doing these once makes dozens of rows below cheap. Each is its own patch.\n")
w("| # | Foundation | Why | Unlocks |\n|---|---|---|---|")
w("| F1 | `src/sources/util.rs`: `strip_noscript`, `html_to_text`, `epoch_to_rfc3339` (no new dependency), `abs_url`, `first_nonempty` | `strip_noscript` currently lives in `bing_videos.rs`; every Bing engine needs it; news engines need ISO dates and ube has no date crate | all Bing engines, every news engine |")
w("| F2 | Shared token cache helper (a `moka` cache keyed by `(engine, key)` with TTL) | vqd, JWT, client_id, secret keys, continuation tokens all need the same pattern (`duckduckgo.rs` already does it ad hoc) | duckduckgo extra, vimeo, soundcloud, pexels, youtube, pinterest, semantic scholar |")
w("| F3 | `GoogleConfig.tbm` (or `google_request` helper) in `google.rs` | Google images/news/videos/scholar differ from web only by the `tbm` parameter and the response parser | google images, news, videos |")
w("| F4 | Generic JSON engine: `JsonEngineConfig { search_url, results_path, url_path, title_path, content_path, thumbnail_path, page_size, first_page, headers }` with slash-separated paths like SearXNG's json_engine | 8 SearXNG instances are pure config | mdn, mankier, packagist, wiby, encyclosearch, crowdview, openaire |")
w("| F5 | Generic HTML engine: same idea with CSS selectors (XPath does not translate 1:1: `concat()`/`substring-after()` need a post-processing closure) | 23 SearXNG instances are pure config | lobste.rs, rubygems, pub.dev, hoogle, anaconda, searchmysite... |")
w("| F6 | MediaWiki engine config (`base_url`, `api_path`, `page_size`, `search_type`) | 12 instances | wikinews, wiktionary, wikibooks, gentoo, nixos... |")
w("| F7 | XML support: **ask the human before adding** `quick-xml` or `roxmltree` | Atom/RSS/XML engines cannot be parsed with `scraper` reliably (HTML parsing mangles `<link>`, namespaces, case) | arxiv, pubmed, bt4g |")
w("| F8 | `wreq` client helper shared by google.rs and any anti-bot engine | brave, qwant, google scholar, yandex need browser TLS/HTTP2 fingerprints | brave, qwant, scholar |\n")
w(f"## Prioritized engines (sorted by score; {len(E)} rows: P0={cnt['P0']}, P1={cnt['P1']}, P2={cnt['P2']}, P3={cnt['P3']}, P4={cnt['P4']})\n")
w("Columns: **cat** = SearXNG category; **ube kind** = where it goes today (IT, science, files and music engines go to `pages` with a low `.weight()` unless the human approves a new kind, see `ENGINES.md`); **types** = see the engine-type section of `ENGINES.md`; **def.** = enabled by default upstream.\n")
w("| # | Tier | Score | Engine (SearXNG) | cat | ube kind | types | effort | def. | Particularities |\n|---|---|---|---|---|---|---|---|---|---|")
esc=lambda s: str(s).replace("|","\\|")
for i,x in enumerate(E,1):
    t = "BLOCKED" if x['eff']=="-" else tier(x['S'])
    w(f"| {i} | {t} | {x['S']} | **{esc(x['id'])}**<br><sub>{esc(x['sx'])}</sub> | {x['cat']} | {x['kind']} | {esc(x['types'])} | {x['eff']} | {x['dflt']} | {esc(x['notes'])} |")
w("\n## Needs a new kind or UI feature first (do not start these without asking)\n")
w("| Group | SearXNG | Needs | Value | Ease | Notes |\n|---|---|---|---|---|---|")
for x in sorted(N,key=lambda x:-(2*x['V']+x['E']+x['R'])):
    w(f"| {esc(x['name'])} | {esc(x['sx'])} | {esc(x['feat'])} | {x['V']} | {x['E']} | {esc(x['notes'])} |")
w("\n## Do not port\n")
w(SKIP+"\n")
import os
OUT=os.path.join(os.path.dirname(os.path.abspath(__file__)),'..','BACKLOG.md')
open(OUT,'w').write(("\n".join(out)).rstrip()+"\n")
print(f'wrote {OUT}: {len(E)} engines', dict(cnt))
