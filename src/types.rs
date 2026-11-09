use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Pages,
    Videos,
    News,
    Forums,
    Images,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Pages => "pages",
            Kind::Videos => "videos",
            Kind::News => "news",
            Kind::Forums => "forums",
            Kind::Images => "images",
        }
    }
}

/// One search hit. Everything kind-specific (thumbnail, duration, published,
/// replies, width/height, ...) goes in `extra`; it is flattened into the JSON
/// so the page templates can bind to it directly, e.g. data-text="duration".
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub icon: Option<String>,
    pub description: String,
    pub score: f64,
    /// Filled in by the server: which sources returned this URL.
    pub sources: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl SearchResult {
    pub fn new(
        title: impl Into<String>,
        url: impl Into<String>,
        description: impl Into<String>,
        score: f64,
    ) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            icon: None,
            description: description.into(),
            score,
            sources: vec![],
            extra: HashMap::new(),
        }
    }
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }
    pub fn with(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Abort everything in flight and forget dedup state (new query / kind).
    Reset,
    /// Load pages `from..=to` of `kind` for `q`.
    Search { epoch: u64, q: String, kind: Kind, from: u32, to: u32 },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Sent before fetching a page: which sources are about to run (drives the side pane).
    Started { epoch: u64, page: u32, sources: Vec<&'static str> },
    /// New or updated (score merged) results from one source for one page.
    Results { epoch: u64, page: u32, source: &'static str, results: Vec<SearchResult> },
    SourceDone {
        epoch: u64,
        page: u32,
        source: &'static str,
        ok: bool,
        cached: bool,
        ms: u64,
        error: Option<String>,
    },
    /// All sources have reported (or failed/timed out) for this page.
    PageDone { epoch: u64, page: u32, errors: u32 },
}
