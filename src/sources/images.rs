use super::{fake, Source};
use crate::types::Kind;

pub fn sources() -> Vec<Source> {
    vec![
        Source::new("images-fast", |q, p| fake(Kind::Images, "images-fast", q, p, 150)),
        Source::new("images-slow", |q, p| fake(Kind::Images, "images-slow", q, p, 6000)),
    ]
}
