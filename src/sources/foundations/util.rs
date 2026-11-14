//! Shared utility functions for HTML scraping and URL handling.
//!
//! These helpers are used by multiple engines, especially those scraping HTML
//! where `<noscript>` tags wrap results or where date conversion is needed.
//!
//! - `strip_noscript`: removes `<noscript>` wrapper tags that some sites use
//!   (e.g., Bing wraps video results in `<noscript>`; lxml in SearXNG parses
//!   this fine but html5ever/scraper treats it as raw text).
//! - `html_to_text`: extracts plain text from HTML, stripping tags and
//!   normalizing whitespace.
//! - `epoch_to_rfc3339`: converts Unix epoch seconds to RFC 3339 UTC format
//!   (ISO 8601), since ube has no date crate.
//! - `abs_url`: resolves a relative URL against a base URL.
//! - `first_nonempty`: returns the first non-empty string from multiple options.

use url::Url;

/// Strip `<noscript>` tags from HTML.
///
/// Some sites (e.g., Bing) wrap their entire result list in `<noscript>`.
/// The `scraper` crate (html5ever) treats `<noscript>` content as raw text,
/// so results would never appear in the DOM. This function removes those
/// wrapper tags, making the contents ordinary elements again.
///
/// Case-insensitive for the tag name; handles `<noscript>`, `<NOSCRIPT>`,
/// `<NoScript>`, etc. Ignores bare `<noscripts>` or similar.
///
/// # Examples
///
/// ```
/// use ube::sources::foundations::util::strip_noscript;
///
/// assert_eq!(
///     strip_noscript("<p>a</p><noscript><b>x</b></noscript><noscripts>"),
///     "<p>a</p><b>x</b><noscripts>"
/// );
/// ```
pub fn strip_noscript(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find('>').map_or(rest.len(), |e| e + 1);
        let tag = &rest[..end];
        let name = tag.trim_start_matches('<').trim_start_matches('/');
        let is_noscript = name.len() >= 8
            && name.is_char_boundary(8)
            && name[..8].eq_ignore_ascii_case("noscript")
            && !name[8..].starts_with(|c: char| c.is_ascii_alphanumeric());
        if !is_noscript {
            out.push_str(tag);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Convert HTML to plain text, stripping all tags and normalizing whitespace.
///
/// This is useful for extracting descriptions/snippets from HTML where the
/// exact tag structure doesn't matter. Collapses multiple whitespace
/// sequences into a single space.
///
/// # Examples
///
/// ```
/// use ube::sources::foundations::util::html_to_text;
///
/// assert_eq!(
///     html_to_text("<p>Hello   world</p><br><span>Test</span>"),
///     "Hello world Test"
/// );
/// ```

/// Convert Unix epoch seconds to RFC 3339 UTC format (ISO 8601).
///
/// ube has no date crate, so this function provides a simple conversion.
/// Assumes the input is valid epoch seconds (since 1970-01-01 00:00:00 UTC).
///
/// # Examples
///
/// ```
/// use ube::sources::foundations::util::epoch_to_rfc3339;
///
/// // Unix epoch: 1970-01-01 00:00:00 UTC
/// assert_eq!(epoch_to_rfc3339(0), Some("1970-01-01T00:00:00Z".to_string()));
/// // 60 seconds later
/// assert_eq!(epoch_to_rfc3339(60), Some("1970-01-01T00:01:00Z".to_string()));
/// ```
pub fn epoch_to_rfc3339(epoch_secs: u64) -> Option<String> {
    const SECONDS_PER_MINUTE: u64 = 60;
    const SECONDS_PER_HOUR: u64 = 3600;
    const SECONDS_PER_DAY: u64 = 86400;

    let days = epoch_secs / SECONDS_PER_DAY;
    let secs_of_day = epoch_secs % SECONDS_PER_DAY;
    let hours = secs_of_day / SECONDS_PER_HOUR;
    let secs_of_hour = secs_of_day % SECONDS_PER_HOUR;
    let minutes = secs_of_hour / SECONDS_PER_MINUTE;
    let seconds = secs_of_hour % SECONDS_PER_MINUTE;

    let mut year = 1970u32;
    let mut remaining_days = days as u32;
    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year as u32;
        year += 1;
    }

    let mut month_start_days = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
        month_start_days[2] = 29;
    }

    let mut month = 1u32;
    loop {
        let days_in_month = month_start_days[month as usize];
        if remaining_days < days_in_month {
            break;
        }
        remaining_days -= days_in_month;
        month += 1;
    }
    let day = remaining_days + 1;

    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z"
    ))
}

/// Resolve a relative URL against a base URL.
///
/// Returns `Ok(absolute_url)` if successful, `Err` if the base URL is invalid.
///
/// # Examples
///
/// ```
/// use ube::sources::foundations::util::abs_url;
/// use url::Url;
///
/// let base = Url::parse("https://example.org/path").unwrap();
/// assert_eq!(abs_url(&base, "/other").unwrap(), "https://example.org/other");
/// assert_eq!(abs_url(&base, "https://other.org/a").unwrap(), "https://other.org/a");
/// ```
pub fn abs_url(base: &Url, relative: &str) -> Result<String, url::ParseError> {
    Ok(base.join(relative)?.to_string())
}

/// Return the first non-empty string from multiple options.
///
/// This is useful when combining multiple possible values where one might
/// be `None` or an empty string. Returns `None` if all options are empty.
///
/// # Examples
///
/// ```
/// use ube::sources::foundations::util::first_nonempty;
///
/// assert_eq!(first_nonempty(Some("a"), Some("b"), None), Some("a"));
/// assert_eq!(first_nonempty(None, Some(""), Some("c")), Some("c"));
/// assert_eq!(first_nonempty(None, Some(""), None), None);
/// ```

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_noscript_removes_wrapper() {
        let html = "<html><body><script>var x = 1;</script><noscript><div><a href=\"/x\">X</a></div></noscript></body></html>";
        let cleaned = strip_noscript(html);
        assert!(cleaned.contains("<div>"));
        assert!(cleaned.contains("<a href"));
        assert!(!cleaned.contains("<noscript><noscript>"));
    }

    #[test]
    fn strip_noscript_case_insensitive() {
        assert!(strip_noscript("<NOSCRIPT>x</NOSCRIPT>").contains("x"));
        assert!(strip_noscript("<NoScript>x</NoScript>").contains("x"));
    }

    #[test]
    fn strip_noscript_preserves_other_tags() {
        assert_eq!(
            strip_noscript("<p>a</p><noscript><b>x</b></noscript><noscripts>"),
            "<p>a</p><b>x</b><noscripts>"
        );
    }

    #[test]
    fn html_to_text_normalizes_whitespace() {
        assert_eq!(
            html_to_text("<p>Hello   world</p><br><span>Test</span>"),
            "Hello world Test"
        );
    }

    #[test]
    fn html_to_text_removes_tags() {
        assert_eq!(html_to_text("<b>Bold</b><i>Italic</i>"), "Bold Italic");
    }

    #[test]
    fn epoch_to_rfc3339_zero() {
        assert_eq!(
            epoch_to_rfc3339(0),
            Some("1970-01-01T00:00:00Z".to_string())
        );
    }

    #[test]
    fn epoch_to_rfc3339_minutes() {
        assert_eq!(
            epoch_to_rfc3339(60),
            Some("1970-01-01T00:01:00Z".to_string())
        );
        assert_eq!(
            epoch_to_rfc3339(3600),
            Some("1970-01-01T01:00:00Z".to_string())
        );
    }

    #[test]
    fn abs_url_resolves_absolute_path() {
        let base = Url::parse("https://example.org/path").unwrap();
        // Absolute path /other replaces the base path
        assert_eq!(
            abs_url(&base, "/other").unwrap(),
            "https://example.org/other"
        );
        // Relative path other is appended to base path
        assert_eq!(
            abs_url(&base, "other").unwrap(),
            "https://example.org/path/other"
        );
        // Absolute URL is used as-is
        assert_eq!(
            abs_url(&base, "https://other.org/a").unwrap(),
            "https://other.org/a"
        );
    }

    #[test]
    fn first_nonempty_skips_empty() {
        assert_eq!(first_nonempty(Some("a"), Some("b"), None), Some("a"));
        assert_eq!(first_nonempty(None, Some(""), Some("c")), Some("c"));
        assert_eq!(first_nonempty(None, Some(""), None), None);
    }
}
pub fn first_nonempty<'a>(
    opt1: Option<&'a str>,
    opt2: Option<&'a str>,
    opt3: Option<&'a str>,
) -> Option<&'a str> {
    // Return the first non-empty string among the options.
    // None or Some("") are skipped.
    if let Some(s) = opt1 {
        if s.is_empty() {
            // Skip empty, check next option
        } else {
            return Some(s);
        }
    }
    if let Some(s) = opt2 {
        if s.is_empty() {
            // Skip empty, check next option
        } else {
            return Some(s);
        }
    }
    if let Some(s) = opt3 {
        if s.is_empty() {
            // Skip empty
        } else {
            return Some(s);
        }
    }
    None
}
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut last_was_text = false;
    let mut in_tag = false;
    let mut tag_name = String::new();
    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            tag_name.clear();
            last_was_text = false;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            tag_name.clear();
            if last_was_text {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
                last_was_text = false;
            }
            continue;
        }
        if in_tag {
            if ch.is_ascii_alphabetic() {
                tag_name.push(ch);
            }
            continue;
        }
        if ch.is_whitespace() {
            if last_was_text {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            last_was_text = true;
        } else {
            if !last_was_text && out.ends_with(' ') {
                out.pop();
            }
            out.push(ch);
            last_was_text = false;
        }
    }
    out
}
