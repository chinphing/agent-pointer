use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default base URL for the annotation service.
const DEFAULT_ANNOTATE_API_BASE: &str = "http://127.0.0.1:8000";
/// Default timeout for annotation requests in seconds.
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
/// Default detection threshold.
const DEFAULT_THRESHOLD: f32 = 0.1;
/// Default overlap threshold.
const DEFAULT_OVERLAP_THRESHOLD: f32 = 0.1;
/// Default padding for bounding boxes.
const DEFAULT_PADDING: i32 = 3;

/// Information about a detected UI element bounding box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxInfo {
    /// The index number shown on the annotated image.
    pub index: u32,
    /// X coordinate of the top-left corner.
    pub x: f32,
    /// Y coordinate of the top-left corner.
    pub y: f32,
    /// Width of the bounding box.
    pub width: f32,
    /// Height of the bounding box.
    pub height: f32,
    /// Detection confidence score.
    pub confidence: f32,
}

impl BoxInfo {
    /// Calculate the center point of the bounding box.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Request payload for the annotation service.
#[derive(Debug, Serialize)]
pub struct AnnotateRequest {
    /// Base64-encoded PNG image.
    pub image: String,
    /// Detection threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    /// Overlap threshold for NMS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_threshold: Option<f32>,
    /// Padding for bounding boxes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<i32>,
}

/// Response from the annotation service.
#[derive(Debug, Deserialize)]
pub struct AnnotateResponse {
    /// Base64-encoded annotated image.
    pub image: String,
    /// Detected bounding boxes.
    pub boxes: Vec<BoxInfo>,
}

/// HTTP client for the UI annotation service.
#[derive(Debug, Clone)]
pub struct AnnotateClient {
    client: Client,
    base_url: String,
}

impl AnnotateClient {
    /// Create a new AnnotateClient with default settings.
    pub fn new() -> Result<Self> {
        Self::with_base_url(DEFAULT_ANNOTATE_API_BASE)
    }

    /// Create a new AnnotateClient with a custom base URL.
    ///
    /// # Arguments
    /// * `base_url` - The base URL of the annotation service.
    pub fn with_base_url(base_url: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// Create a new AnnotateClient from environment variables.
    /// Uses `COMPUTER_ANNOTATE_API_BASE` for the base URL if set.
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("COMPUTER_ANNOTATE_API_BASE")
            .unwrap_or_else(|_| DEFAULT_ANNOTATE_API_BASE.to_string());
        Self::with_base_url(&base_url)
    }

    /// Send an image to the annotation service and get back annotated results.
    ///
    /// # Arguments
    /// * `image_bytes` - Raw PNG image bytes.
    ///
    /// # Returns
    /// A tuple of (annotated_image_bytes, detected_boxes).
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails or the response is invalid.
    pub async fn annotate_image(&self, image_bytes: &[u8]) -> Result<(Vec<u8>, Vec<BoxInfo>)> {
        let request = AnnotateRequest {
            image: crate::tools::computer::screen::encode_image_to_base64(image_bytes),
            threshold: Some(DEFAULT_THRESHOLD),
            overlap_threshold: Some(DEFAULT_OVERLAP_THRESHOLD),
            padding: Some(DEFAULT_PADDING),
        };

        let url = format!("{}/api/v1/annotate/all", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("Annotation request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Annotation service returned error ({}): {}",
                status,
                text
            ));
        }

        let annotate_response: AnnotateResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse annotation response: {}", e))?;

        let image_bytes = decode_base64_image(&annotate_response.image)?;

        Ok((image_bytes, annotate_response.boxes))
    }
}

impl Default for AnnotateClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default AnnotateClient")
    }
}

/// Decode a base64-encoded image string to raw bytes.
fn decode_base64_image(encoded: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(encoded)
        .map_err(|e| anyhow!("Failed to decode base64 image: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_info_center() {
        let box_info = BoxInfo {
            index: 1,
            x: 100.0,
            y: 200.0,
            width: 50.0,
            height: 30.0,
            confidence: 0.95,
        };
        let (cx, cy) = box_info.center();
        assert_eq!(cx, 125.0);
        assert_eq!(cy, 215.0);
    }

    #[test]
    fn test_client_default_url() {
        let client = AnnotateClient::new().unwrap();
        assert_eq!(client.base_url, DEFAULT_ANNOTATE_API_BASE);
    }

    #[test]
    fn test_client_custom_url() {
        let client = AnnotateClient::with_base_url("http://localhost:9000/").unwrap();
        assert_eq!(client.base_url, "http://localhost:9000");
    }
}
