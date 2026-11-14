use super::{fake, Source};
use crate::sources::engines::stackexchange;
use crate::types::Kind;
pub fn sources() -> Vec<Source> {
    vec![
        Source::new("stackexchange", stackexchange::stackexchange_pages),
        // Placeholder sources for testing the streaming UI:
        // Source::new("forums-fast", |q, p| fake(Kind::Forums, "forums-fast", q, p, 150)),
        //Source::new("forums-slow", |q, p| fake(Kind::Forums, "forums-slow", q, p, 6000)),
    ]
}
