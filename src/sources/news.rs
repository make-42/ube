use super::{fake, Source};
use crate::types::Kind;

pub fn sources() -> Vec<Source> {
    vec![
        Source::new("news-fast", |q, p| fake(Kind::News, "news-fast", q, p, 150)),
        Source::new("news-slow", |q, p| fake(Kind::News, "news-slow", q, p, 6000)),
    ]
}
