//! Foundation utilities for HTML scraping, URL handling, and Google-specific helpers.
//! See [`google`] for Google tab-specific search helpers (F3 foundation).
//! See [`json_engine`] for a generic JSON engine (F4 foundation).
//! See [`cache`] for token caching for stateful pagination (e.g., DuckDuckGo, Vimeo, Pinterest).
//! See [`util`] for:
//! - `strip_noscript`: removes `<noscript>` wrapper tags used by some sites (e.g., Bing)
//! - `html_to_text`: extracts plain text from HTML
//! - `epoch_to_rfc3339`: converts Unix epoch seconds to RFC 3339 UTC format
//! - `abs_url`: resolves relative URLs against a base URL
//! - `first_nonempty`: returns the first non-empty string from an iterator

pub mod cache;
pub mod google;
pub mod json_engine;
pub mod util;
