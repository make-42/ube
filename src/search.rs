use crate::{
    sources::Source,
    state::AppState,
    types::{Kind, SearchResult, ServerMsg},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::{sync::mpsc, task::JoinSet};

/// Per-connection dedup state: normalized url -> (page it first appeared on, merged result).
pub type Seen = Arc<Mutex<HashMap<String, (u32, SearchResult)>>>;

pub fn normalize_query(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

// Crude on purpose; swap for the `url` crate if you need canonicalization.
fn url_key(u: &str) -> String {
    u.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_lowercase()
}

/// Merge one source's list into the session. Returns the results to send:
/// new URLs as-is, already-seen URLs with their score summed.
fn merge(seen: &Seen, page: u32, source: &'static str, list: &[SearchResult]) -> Vec<SearchResult> {
    let mut seen = seen.lock().unwrap();
    let mut out = Vec::new();
    for r in list {
        let key = url_key(&r.url);
        match seen.get_mut(&key) {
            Some((p, existing)) => {
                if *p != page || existing.sources.iter().any(|s| s == source) {
                    continue; // other page already owns it, or this source already counted
                }
                existing.score += r.score; // <- your scoring/merging logic goes here
                existing.sources.push(source.to_string());
                out.push(existing.clone());
            }
            None => {
                let mut r = r.clone();
                if !r.score.is_finite() {
                    r.score = 0.0;
                }
                r.sources = vec![source.to_string()];
                seen.insert(key, (page, r.clone()));
                out.push(r);
            }
        }
    }
    out
}

async fn fetch(
    state: &AppState,
    src: &Source,
    kind: Kind,
    q: &str,
    page: u32,
) -> (Result<Arc<Vec<SearchResult>>, String>, bool) {
    let key = (kind, q.to_string(), page, src.name);
    let timeout = state.cfg.source_timeout;
    let (f, qq) = (src.fetch.clone(), q.to_string());
    // or_try_insert_with also coalesces identical concurrent fetches, and errors aren't cached.
    let res = state
        .cache
        .entry(key)
        .or_try_insert_with(async move {
            match tokio::time::timeout(timeout, f(qq, page)).await {
                Ok(Ok(v)) => Ok(Arc::new(v)),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err(format!("timed out after {}ms", timeout.as_millis())),
            }
        })
        .await;
    match res {
        Ok(entry) => {
            let cached = !entry.is_fresh();
            (Ok(entry.into_value()), cached)
        }
        Err(e) => (Err((*e).clone()), false),
    }
}

pub async fn run_search(
    state: Arc<AppState>,
    out: mpsc::Sender<ServerMsg>,
    seen: Seen,
    epoch: u64,
    q: String,
    kind: Kind,
    from: u32,
    to: u32,
) {
    let q = normalize_query(&q);
    if q.is_empty() {
        return;
    }
    let to = to.clamp(1, state.cfg.max_pages);
    let from = from.clamp(1, to);
    let sources = state.sources.get(&kind).cloned().unwrap_or_default();

    let mut remaining: HashMap<u32, (usize, u32)> = HashMap::new(); // page -> (sources left, errors)
    let mut set = JoinSet::new();
    for page in from..=to {
        remaining.insert(page, (sources.len(), 0));
        let names = sources.iter().map(|s| s.name).collect();
        if out.send(ServerMsg::Started { epoch, page, sources: names }).await.is_err() {
            return;
        }
        if sources.is_empty() {
            let _ = out.send(ServerMsg::PageDone { epoch, page, errors: 0 }).await;
        }
        for src in &sources {
            let (state, src, q) = (state.clone(), src.clone(), q.clone());
            set.spawn(async move {
                let t = Instant::now();
                let (res, cached) = fetch(&state, &src, kind, &q, page).await;
                (page, src.name, res, cached, t.elapsed().as_millis() as u64)
            });
        }
    }

    // Every source runs independently; results are forwarded the moment each one lands.
    while let Some(joined) = set.join_next().await {
        let Ok((page, source, res, cached, ms)) = joined else { continue };
        let (ok, error, batch) = match res {
            Ok(list) => (true, None, merge(&seen, page, source, &list)),
            Err(e) => (false, Some(e), Vec::new()),
        };
        if !batch.is_empty()
            && out.send(ServerMsg::Results { epoch, page, source, results: batch }).await.is_err()
        {
            return;
        }
        if out
            .send(ServerMsg::SourceDone { epoch, page, source, ok, cached, ms, error })
            .await
            .is_err()
        {
            return;
        }
        let (left, errors) = {
            let e = remaining.get_mut(&page).unwrap();
            e.0 -= 1;
            if !ok {
                e.1 += 1;
            }
            *e
        };
        if left == 0 && out.send(ServerMsg::PageDone { epoch, page, errors }).await.is_err() {
            return;
        }
    }
}
