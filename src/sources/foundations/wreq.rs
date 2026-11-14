//! Anti-bot HTTP client helper (F8 foundation).
//!
//! Provides a shared `wreq` client with Chrome 100 Android TLS/HTTP2 fingerprint
//! and emulation profile. This is used by engines that need browser-like fingerprints
//! to bypass anti-bot measures (Brave, Qwant, Google Scholar, Yandex, etc.).
//!
//! The client uses `wreq-util`'s `Profile::Chrome100` and `Platform::Android`
//! to emulate Chrome 100 on Android, which is the closest available profile
//! to SearXNG's Chrome 99 Android configuration.
//!
//! # Usage
//!
//! ```ignore
//! use crate::sources::foundations::wreq::client;
//!
//! // Returns a shared static client
//! let static_client = client();
//!
//! // Or build_url() for custom timeout
//! let client = build_wreq_client(Some(Duration::from_secs(5)));
//! ```

use anyhow::Context;
use std::sync::OnceLock;
use std::time::Duration;
use wreq::Response;
use wreq_util::{Emulation, Platform, Profile};

/// Build a wreq client with Chrome 100 Android emulation.
///
/// # Arguments
///
/// * `timeout` - Optional timeout override (default: 15 seconds)
///
/// # Returns
///
/// A `wreq::Client` configured with Chrome 100 Android TLS/HTTP2 fingerprint.
///
/// # Example
///
/// ```ignore
/// let client = build_wreq_client(None);
/// let resp = client.get("https://example.com").send().await?;
/// let body = resp.text().await?;
/// ```
pub fn build_wreq_client(timeout: Option<Duration>) -> wreq::Client {
    let emulation = Emulation::builder()
        .profile(Profile::Chrome100)
        .platform(Platform::Android)
        .build();

    wreq::Client::builder()
        .timeout(timeout.unwrap_or(Duration::from_secs(15)))
        .emulation(emulation)
        .build()
        .expect("failed to build wreq client")
}

/// Shared static wreq client.
///
/// Returns a singleton client instance with Chrome 100 Android emulation.
///
/// # Panics
///
/// Panics on first call if the client cannot be created (should never happen).
pub fn client() -> &'static wreq::Client {
    static CLIENT: OnceLock<wreq::Client> = OnceLock::new();
    CLIENT.get_or_init(|| build_wreq_client(None))
}

/// Execute a GET request with the shared wreq client.
///
/// # Returns
///
/// `Ok(Response)` on success, `Err(anyhow::Error)` on failure.
///
/// # Example
///
/// ```ignore
/// use crate::sources::foundations::wreq;
///
/// let resp = wreq::get("https://example.com").await?;
/// let body = resp.text().await?;
/// ```
pub async fn get(url: &str) -> Result<Response, anyhow::Error> {
    let client = client();
    let resp = client
        .get(url)
        .send()
        .await
        .context("wreq GET request failed")?;

    Ok(resp)
}

/// Execute a POST request with the shared wreq client.
///
/// # Arguments
///
/// * `url` - The request URL
/// * `body` - Optional JSON body as bytes
///
/// # Returns
///
/// `Ok(Response)` on success, `Err(anyhow::Error)` on failure.
///
/// # Example
///
/// ```ignore
/// use crate::sources::foundations::wreq;
/// use serde_json::json;
///
/// let body = serde_json::to_vec(&json!({ "q": "test" })).unwrap();
/// let resp = wreq::post("https://api.example.com/search", &body).await?;
/// ```
pub async fn post(url: &str, body: Option<&[u8]>) -> Result<Response, anyhow::Error> {
    let client = client();
    
    let resp = if let Some(b) = body {
        // Body is owned by caller, move it into the request
        client.post(url).body(b.to_vec()).send().await
    } else {
        client.post(url).send().await
    };

    resp.context("wreq POST request failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wreq::Response;

    /// Test that the shared client is properly initialized.
    #[test]
    fn client_is_initialized() {
        let _ = client(); // Should not panic
    }

    /// Test that build_wreq_client returns a valid client.
    #[test]
    fn build_wreq_client_is_valid() {
        let client = build_wreq_client(None);
        // Just verify we can build it without panicking
        // timeout() is not available on wreq::Client, just verify we can build it
        assert!(true);
    }

    /// Test that get() returns a response.
    #[tokio::test]
    #[ignore] // Requires network
    async fn test_get_example() {
        // Test with a known good endpoint
        let resp: Result<Response, _> = get("https://httpbin.org/get").await;
        assert!(resp.is_ok());
        let body = resp.unwrap().text().await.unwrap();
        assert!(body.contains("\"origin\""));
    }

    /// Test that post() returns a response.
    #[tokio::test]
    #[ignore] // Requires network
    async fn test_post_example() {
        let body = b"test";
        let resp: Result<Response, _> = post("https://httpbin.org/post", Some(body)).await;
        assert!(resp.is_ok());
        let body = resp.unwrap().text().await.unwrap();
        assert!(body.contains("\"data\""));
    }
}
