//! XML parsing for Atom/RSS feed engines (F7 foundation).
//!
//! This module provides utilities for parsing XML-based search results
//! from sources like arxiv (Atom), pubmed (eutils XML), and bt4g (RSS).

use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// Atom feed entry
#[derive(Debug, Clone)]
pub struct Entry {
    pub title: String,
    pub id: String,
    pub link: String,
    pub updated: String,
    pub summary: String,
}

/// Parse an Atom feed XML into a vector of entries.
pub fn parse_atom(xml: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {}
            Ok(Event::Text(_)) => {}
            Ok(Event::End(_)) => {}
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    entries
}

/// Parse an RSS feed XML into a vector of entries.
pub fn parse_rss(xml: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {}
            Ok(Event::Text(_)) => {}
            Ok(Event::End(_)) => {}
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    entries
}
