use super::{engines::dockerhub, engines::google, engines::duckduckgo, Source};

pub fn sources() -> Vec<Source> {
    vec![
        Source::new("google", google::google_pages),
        Source::new("duckduckgo", duckduckgo::duckduckgo_pages),
        // Weight < 1: web results outrank Docker Hub at equal rank. Override at runtime with
        // SOURCE_WEIGHTS="dockerhub=1.5" (or any `name=weight` list).
        Source::new("dockerhub", dockerhub::dockerhub_pages).weight(0.3),
        // Same source, other region/language (see GoogleConfig in google.rs):
        // Source::new("google-fr", |q, p| {
        //     google::google_with(google::GoogleConfig { hl: "fr".into(), lr: "lang_fr".into(), ..Default::default() }, q, p)
        // }),
        //
        // Placeholder sources for testing the streaming UI:
        // Source::new("pages-fast", |q, p| super::fake(crate::types::Kind::Pages, "pages-fast", q, p, 150)),
        // Source::new("pages-slow", |q, p| super::fake(crate::types::Kind::Pages, "pages-slow", q, p, 6000)),
    ]
}
