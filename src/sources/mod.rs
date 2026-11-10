//! A source is just an async fn `(query, page) -> Result<Vec<SearchResult>>`.
//! Write one, then add it to the `sources()` list of the kind it serves.

pub mod forums;
pub mod engines;
pub mod images;
pub mod news;
pub mod pages;
pub mod videos;

use crate::types::{Kind, SearchResult};
use futures_util::future::BoxFuture;
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

pub type FetchFn =
    Arc<dyn Fn(String, u32) -> BoxFuture<'static, anyhow::Result<Vec<SearchResult>>> + Send + Sync>;

#[derive(Clone)]
pub struct Source {
    pub name: &'static str,
    pub fetch: FetchFn,
    /// Multiplies every score this source contributes (default 1.0). Overridable
    /// at runtime with `SOURCE_WEIGHTS="name=weight,..."`, see `Config`.
    pub weight: f64,
}

impl Source {
    pub fn new<F, Fut>(name: &'static str, f: F) -> Self
    where
        F: Fn(String, u32) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<Vec<SearchResult>>> + Send + 'static,
    {
        Self { name, fetch: Arc::new(move |q, p| Box::pin(f(q, p))), weight: 1.0 }
    }

    /// Rank this source's results higher (>1) or lower (<1) than the others'.
    pub fn weight(mut self, weight: f64) -> Self {
        self.weight = weight;
        self
    }
}

pub fn registry() -> HashMap<Kind, Vec<Source>> {
    HashMap::from([
        (Kind::Pages, pages::sources()),
        (Kind::Videos, videos::sources()),
        (Kind::News, news::sources()),
        (Kind::Forums, forums::sources()),
        (Kind::Images, images::sources()),
    ])
}

/// Placeholder source: sleeps, then returns 10 fake results for `page`.
/// Even-ranked URLs are shared between sources so you can watch scores merge.
pub async fn fake(
    kind: Kind,
    name: &'static str,
    q: String,
    page: u32,
    delay_ms: u64,
) -> anyhow::Result<Vec<SearchResult>> {
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    let slug = q.replace(' ', "-");
    let k = kind.as_str();
    Ok((0..10u32)
        .map(|i| {
            let rank = (page - 1) * 10 + i;
            let url = if i % 2 == 0 {
                format!("https://example.com/{k}/{slug}/{rank}")
            } else {
                format!("https://{name}.example.com/{k}/{slug}/{rank}")
            };
            let r = SearchResult::new(
                format!("[{name}] {q} - {k} result {rank}"),
                url,
                format!("Placeholder description from {name}."),
                1.0 / (1.0 + rank as f64),
            )
            .icon("https://example.com/favicon.ico");
            match kind {
                Kind::Pages => r,
                Kind::Images => r
                    .with("thumbnail", format!("https://example.com/thumb/{rank}.jpg"))
                    .with("width", 640)
                    .with("height", 480),
                Kind::Videos => r
                    .with("thumbnail", format!("https://example.com/thumb/{rank}.jpg"))
                    .with("duration", "4:20"),
                Kind::News => r.with("published", "2026-11-09T08:00:00Z"),
                Kind::Forums => r.with("replies", 12),
            }
        })
        .collect())
}
