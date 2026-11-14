use super::engines::wikipedia;
use super::engines::google;
use super::engines::github;
use super::engines::duckduckgo;
use super::engines::dockerhub;
use super::engines::mdn;
use crate::sources::Source;

pub fn sources() -> Vec<Source> {
    vec![
        Source::new("google", google::google_pages),
        Source::new("duckduckgo", duckduckgo::duckduckgo_pages),
        // Weight < 1: web results outrank Docker Hub at equal rank. Override at runtime with
        // SOURCE_WEIGHTS="dockerhub=1.5" (or any `name=weight` list).
        Source::new("dockerhub", dockerhub::dockerhub_pages).weight(0.3),
        Source::new("github", github::github_pages).weight(0.3),
        // Same source, other region/language (see GoogleConfig in google.rs):
        // Source::new("google-fr", |q, p| {
        //     google::google_with(google::GoogleConfig { hl: "fr".into(), lr: "lang_fr".into(), ..Default::default() }, q, p)
        // }),
        //
        // Placeholder sources for testing the streaming UI:
        // Source::new("pages-fast", |q, p| super::fake(crate::types::Kind::Pages, "pages-fast", q, p, 150)),
        // Source::new("pages-slow", |q, p| super::fake(crate::types::Kind::Pages, "pages-slow", q, p, 6000)),
        //
        // MDN Web Docs (Generic JSON engine):
        Source::new("mdn", mdn::mdn_pages),
        Source::new("wikipedia", wikipedia::wikipedia_pages),
    ]
}
