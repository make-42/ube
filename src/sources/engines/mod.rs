//! Reusable scrapers for individual search engines. One file per engine; each
//! exposes async fns that the per-kind files (pages.rs, videos.rs, ...) register.

pub mod dockerhub;
pub mod google;
pub mod duckduckgo;
