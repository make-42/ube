mod search;
mod sources;
mod state;
mod types;
mod ws;

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use state::{AppState, Config};
use std::sync::Arc;
use tower_http::compression::CompressionLayer;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cfg = Config::from_env();
    let bind = cfg.bind.clone();
    let app = Arc::new(AppState::new(cfg));

    let router = Router::new()
        .route("/", get(|| async { Redirect::to("/search") }))
        .route("/search", get(search_page).layer(CompressionLayer::new()))
        .route("/ws", get(ws::ws_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    tracing::info!("listening on http://{bind}/search?q=hello");
    axum::serve(listener, router).await.unwrap();
}

/// The page is the same static shell for every query (the query is read
/// client-side), so one cached copy serves every search. ETag => cheap 304s.
async fn search_page(State(app): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let p = &app.page;
    let hdrs = || [(header::CACHE_CONTROL, p.cache_control.clone()), (header::ETAG, p.etag.clone())];
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(p.etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, hdrs()).into_response();
    }
    (hdrs(), Html(p.html)).into_response()
}
