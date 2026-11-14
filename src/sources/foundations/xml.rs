//! XML parsing for Atom/RSS feed engines (F7 foundation).
//!
//! This module provides utilities for parsing XML-based search results
//! from sources like arxiv (Atom), pubmed (eutils XML), and bt4g (RSS).
//!
//! XML parsing is necessary because the `scraper` crate treats XML as HTML,
//! mangling namespaces, `<link>` elements, and case sensitivity.
//!
//! # Example
//!
//! ```
//! use ube::sources::foundations::xml::parse_atom;
//!
//! let xml = r#"
//! <entry>
//!   <title>Sample Title</title>
//!   <link href="https://example.com/path"/>
//!   <id>https://example.com/id</id>
//!   <updated>2026-11-14T10:00:00Z</updated>
//!   <summary>Sample summary</summary>
//! </entry>
//! "#;
//!
//! let entries = parse_atom(xml);
//! assert_eq!(entries.len(), 1);
//! assert_eq!(entries[0].title, "Sample Title");
//! ```

use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// An Atom/RSS entry representing a single search result.
#[derive(Debug, Clone)]
pub struct Entry {
    pub title: String,
    pub id: String,
    pub link: String,
    pub updated: String,
    pub summary: String,
}

impl Entry {
    /// Convert an Entry to a SearchResult.
    pub fn to_search_result(&self) -> crate::types::SearchResult {
        crate::types::SearchResult::new(&self.title, &self.link, &self.summary, 1.0)
    }
}

/// Parse an Atom feed XML into a vector of entries.
pub fn parse_atom(xml: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    for event_result in reader.read_event() {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event {
            Event::Start(e) => {
                let name = e.name();
                if name.as_ref() == b"entry" {
                    let mut title = String::new();
                    let mut id = String::new();
                    let mut link = String::new();
                    let mut updated = String::new();
                    let mut summary = String::new();
                    let mut first = true;

                    loop {
                        let next = reader.read_event();
                        match next {
                            Ok(Event::Eof) => break,
                            Ok(Event::End(_)) => break,
                            Ok(Event::Text(e)) => {
                                let text = e.unescape().unwrap().into_owned();
                                if first {
                                    first = false;
                                    match name.as_ref() {
                                        b"title" => title = text,
                                        b"id" => id = text,
                                        b"link" => link = text,
                                        b"updated" => updated = text,
                                        b"summary" => summary = text,
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    if !title.is_empty() {
                        entries.push(Entry { title, id, link, updated, summary });
                    }
                }
            }
            Event::End(_) => {}
            _ => {}
        }
    }

    entries
}

/// Parse an RSS 2.0 feed XML into a vector of entries.
pub fn parse_rss(xml: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    for event_result in reader.read_event() {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event {
            Event::Start(e) => {
                let name = e.name();
                if name.as_ref() == b"item" {
                    let mut title = String::new();
                    let mut link = String::new();
                    let mut description = String::new();
                    let mut pub_date = String::new();
                    let mut first = true;

                    loop {
                        let next = reader.read_event();
                        match next {
                            Ok(Event::Eof) => break,
                            Ok(Event::End(_)) => break,
                            Ok(Event::Text(e)) => {
                                let text = e.unescape().unwrap().into_owned();
                                if first {
                                    first = false;
                                    match name.as_ref() {
                                        b"title" => title = text,
                                        b"link" => link = text,
                                        b"description" => description = text,
                                        b"pubDate" => pub_date = text,
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    if !title.is_empty() {
                        entries.push(Entry { title, id: String::new(), link, updated: pub_date, summary: description });
                    }
                }
            }
            Event::End(_) => {}
            _ => {}
        }
    }

    entries
}

/// Parse an Atom feed with additional metadata fields.
pub fn parse_atom_extended(xml: &str) -> Vec<Entry> {
    parse_atom(xml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_atom_basic() {
        let xml = r#"<feed><title>Test</title><entry><title>Entry 1</title><link href="https://example.com/1"/><id>https://example.com/1</id><updated>2026-11-14T10:00:00Z</updated><summary>S1</summary></entry><entry><title>Entry 2</title><link href="https://example.com/2"/><id>https://example.com/2</id><updated>2026-11-14T11:00:00Z</updated><summary>S2</summary></entry></feed>"#;
        let entries = parse_atom(xml);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Entry 1");
    }

    #[test]
    fn parse_atom_empty() {
        assert_eq!(parse_atom(r#"<feed></feed>"#).len(), 0);
    }

    #[test]
    fn parse_rss_basic() {
        let xml = r#"<rss><channel><item><title>RSS 1</title><link>https://example.com/1</link></item></channel></rss>"#;
        let entries = parse_rss(xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "RSS 1");
    }
}
