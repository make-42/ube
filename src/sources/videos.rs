use super::{fake, Source};
use crate::types::Kind;

pub fn sources() -> Vec<Source> {
    vec![
        Source::new("videos-fast", |q, p| fake(Kind::Videos, "videos-fast", q, p, 150)),
        Source::new("videos-slow", |q, p| fake(Kind::Videos, "videos-slow", q, p, 6000)),
    ]
}
