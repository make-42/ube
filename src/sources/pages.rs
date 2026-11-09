use super::{engines::google, engines::duckduckgo, Source};

pub fn sources() -> Vec<Source> {
    vec![
        Source::new("google", google::google_pages),
        Source::new("duckduckgo", duckduckgo::duckduckgo_pages),
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
