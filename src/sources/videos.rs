use super::{engines::bing_videos, Source};

pub fn sources() -> Vec<Source> {
    vec![
        Source::new("bing-videos", bing_videos::bing_videos),
        // Same source, other market / time range (see BingVideosConfig in bing_videos.rs):
        // Source::new("bing-videos-fr", |q, p| {
        //     bing_videos::bing_videos_with(
        //         bing_videos::BingVideosConfig { market: Some("fr-FR".into()), ..Default::default() },
        //         q,
        //         p,
        //     )
        // }),
        //
        // Placeholder sources for testing the streaming UI:
        // Source::new("videos-fast", |q, p| super::fake(crate::types::Kind::Videos, "videos-fast", q, p, 150)),
        // Source::new("videos-slow", |q, p| super::fake(crate::types::Kind::Videos, "videos-slow", q, p, 6000)),
    ]
}
