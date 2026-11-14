pub mod html_engine;
// Reusable scrapers for individual search engines. One file per engine; each
// exposes async fns that the per-kind files (pages.rs, videos.rs, ...) register.
pub mod json_engine;
pub mod mediawiki;
pub mod bing_videos;
pub mod dockerhub;
pub mod google;
pub mod duckduckgo;
