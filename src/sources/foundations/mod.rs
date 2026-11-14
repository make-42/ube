//! Foundation utilities for HTML scraping and URL handling.
//!
//! These modules provide shared helper functions that are used by multiple engines,
//! particularly those scraping HTML where `<noscript>` tags wrap results or where
//! date conversion is needed.
//!
//! See [`util`] for:
//! - `strip_noscript`: removes `<noscript>` wrapper tags used by some sites (e.g., Bing)
//! - `html_to_text`: extracts plain text from HTML
//! - `epoch_to_rfc3339`: converts Unix epoch seconds to RFC 3339 UTC format
//! - `abs_url`: resolves relative URLs against a base URL
//! - `first_nonempty`: returns the first non-empty string from an iterator

pub mod util;
