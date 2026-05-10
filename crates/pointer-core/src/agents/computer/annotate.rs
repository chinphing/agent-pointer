use anyhow::{anyhow, Result};
use reqwest::multipart;
use reqwest::Client;
use serde::Deserialize;
use std::io::Cursor;
use std::time::{Duration, Instant};

/// Default base URL for the annotation service.
const DEFAULT_ANNOTATE_API_BASE: &str = "http://127.0.0.1:8000";
/// Default timeout for annotation requests in seconds.
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
/// Default detection threshold.
const DEFAULT_THRESHOLD: f32 = 0.1;
/// Default overlap threshold.
const DEFAULT_OVERLAP_THRESHOLD: f32 = 0.1;
/// Default padding for bounding boxes (per API).
const DEFAULT_PADDING: i32 = 3;
/// Scale rule: `max(width, height)` of the upload must not exceed this (px).
const CAP_ANNOTATE_MAX_LONG_EDGE: u32 = 1920;
/// Optional override to **reduce** size (e.g. avoid 413); clamped to `[320, CAP_ANNOTATE_MAX_LONG_EDGE]`.
const ENV_ANNOTATE_MAX_LONG_EDGE: &str = "COMPUTER_ANNOTATE_MAX_LONG_EDGE";

/// Information about a detected UI element bounding box.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// `POST /api/v1/annotate/all` JSON body (success).
#[derive(Debug, Deserialize)]
struct AnnotateAllResponse {
    /// `[x1, y1, x2, y2]` per box, order matches labels `1…N` on the image.
    boxes: Vec<Vec<f64>>,
    /// Raw base64 (**no** `data:image/...` prefix).
    image_base64: String,
    #[allow(dead_code)]
    #[serde(default)]
    mime_type: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    width: Option<i32>,
    #[allow(dead_code)]
    #[serde(default)]
    height: Option<i32>,
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
        let t_total = Instant::now();

        let max_edge = std::env::var(ENV_ANNOTATE_MAX_LONG_EDGE)
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .map(|n| n.clamp(320, CAP_ANNOTATE_MAX_LONG_EDGE))
            .unwrap_or(CAP_ANNOTATE_MAX_LONG_EDGE);

        let t = Instant::now();
        let PreparedUpload {
            png_bytes,
            scale_x,
            scale_y,
        } = prepare_png_for_annotation_upload(image_bytes, max_edge)?;
        let prepare_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let file_part = multipart::Part::bytes(png_bytes)
            .file_name("screen.png")
            .mime_str("image/png")
            .map_err(|e| anyhow!("annotate multipart file part: {}", e))?;
        let form = multipart::Form::new()
            .part("file", file_part)
            .text("threshold", format!("{}", DEFAULT_THRESHOLD))
            .text("overlap_threshold", format!("{}", DEFAULT_OVERLAP_THRESHOLD))
            .text("padding", format!("{}", DEFAULT_PADDING));
        let form_ms = t.elapsed().as_secs_f64() * 1000.0;

        let url = format!("{}/api/v1/annotate/all", self.base_url);
        let t = Instant::now();
        let response = self
            .client
            .post(&url)
            .multipart(form)
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

        let annotate_response: AnnotateAllResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse annotation response: {}", e))?;
        let http_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let boxes_api = boxes_xyxy_to_box_infos(&annotate_response.boxes)?;
        let image_bytes = decode_base64_image(strip_data_url_prefix(&annotate_response.image_base64))?;
        let boxes = scale_boxes_to_capture_space(boxes_api, scale_x, scale_y);
        let decode_ms = t.elapsed().as_secs_f64() * 1000.0;

        log::info!(
            "annotate_image: prepare_downscale {:.1}ms, build_multipart {:.1}ms, post+parse_json {:.1}ms, decode_boxes_png {:.1}ms, total {:.1}ms ({} boxes, max_edge={})",
            prepare_ms,
            form_ms,
            http_ms,
            decode_ms,
            t_total.elapsed().as_secs_f64() * 1000.0,
            boxes.len(),
            max_edge
        );

        Ok((image_bytes, boxes))
    }
}

impl Default for AnnotateClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default AnnotateClient")
    }
}

fn boxes_xyxy_to_box_infos(rows: &[Vec<f64>]) -> Result<Vec<BoxInfo>> {
    let mut out = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        if row.len() != 4 {
            return Err(anyhow!(
                "annotate boxes: expected [x1,y1,x2,y2], got len {}: {:?}",
                row.len(),
                row
            ));
        }
        let x1 = row[0] as f32;
        let y1 = row[1] as f32;
        let x2 = row[2] as f32;
        let y2 = row[3] as f32;
        let width = (x2 - x1).max(0.0);
        let height = (y2 - y1).max(0.0);
        out.push(BoxInfo {
            index: (i + 1) as u32,
            x: x1,
            y: y1,
            width,
            height,
            confidence: 1.0,
        });
    }
    Ok(out)
}

/// Strip optional `data:image/...;base64,` prefix if the server ever sends it.
fn strip_data_url_prefix(s: &str) -> &str {
    s.strip_prefix("data:")
        .and_then(|rest| rest.split_once(',').map(|(_, b64)| b64))
        .unwrap_or(s)
        .trim()
}

struct PreparedUpload {
    png_bytes: Vec<u8>,
    /// Multiply box coordinates from the service by these to map into capture / monitor-local pixels.
    scale_x: f32,
    scale_y: f32,
}

/// Downscale if `max(w,h) > max_long_edge` so the upload respects the long-edge cap.
fn prepare_png_for_annotation_upload(bytes: &[u8], max_long_edge: u32) -> Result<PreparedUpload> {
    let img = image::load_from_memory(bytes).map_err(|e| anyhow!("decode screenshot for annotate: {}", e))?;
    let (w0, h0) = (img.width(), img.height());
    let m = w0.max(h0);
    let (img, scale_x, scale_y) = if m <= max_long_edge {
        (img, 1.0_f32, 1.0_f32)
    } else {
        let scale = max_long_edge as f32 / m as f32;
        let w1 = ((w0 as f32) * scale).round().max(1.0) as u32;
        let h1 = ((h0 as f32) * scale).round().max(1.0) as u32;
        log::debug!(
            "annotate upload downscaled {}x{} -> {}x{} (max_long_edge={})",
            w0,
            h0,
            w1,
            h1,
            max_long_edge
        );
        let resized = img.resize(w1, h1, image::imageops::FilterType::Triangle);
        let scale_x = w0 as f32 / w1 as f32;
        let scale_y = h0 as f32 / h1 as f32;
        (resized, scale_x, scale_y)
    };

    let mut png_bytes = Vec::new();
    {
        let mut cursor = Cursor::new(&mut png_bytes);
        img.write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| anyhow!("encode PNG for annotate: {}", e))?;
    }

    Ok(PreparedUpload {
        png_bytes,
        scale_x,
        scale_y,
    })
}

fn scale_boxes_to_capture_space(boxes: Vec<BoxInfo>, scale_x: f32, scale_y: f32) -> Vec<BoxInfo> {
    if (scale_x - 1.0).abs() < f32::EPSILON && (scale_y - 1.0).abs() < f32::EPSILON {
        return boxes;
    }
    boxes
        .into_iter()
        .map(|mut b| {
            b.x *= scale_x;
            b.y *= scale_y;
            b.width *= scale_x;
            b.height *= scale_y;
            b
        })
        .collect()
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

    #[test]
    fn boxes_xyxy_to_box_infos_order_and_geometry() {
        let rows = vec![vec![0.0, 0.0, 10.0, 20.0], vec![5.0, 5.0, 15.0, 25.0]];
        let out = super::boxes_xyxy_to_box_infos(&rows).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].index, 1);
        assert_eq!(out[0].x, 0.0);
        assert_eq!(out[0].width, 10.0);
        assert_eq!(out[0].height, 20.0);
        assert_eq!(out[1].index, 2);
    }

    #[test]
    fn strip_data_url_prefix_optional() {
        assert_eq!(super::strip_data_url_prefix("abcd"), "abcd");
        assert_eq!(
            super::strip_data_url_prefix("data:image/png;base64,XYZ"),
            "XYZ"
        );
    }

    #[test]
    fn scale_boxes_maps_back_from_downscaled_space() {
        let boxes = vec![BoxInfo {
            index: 1,
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
            confidence: 1.0,
        }];
        let out = super::scale_boxes_to_capture_space(boxes, 2.0, 2.0);
        assert_eq!(out[0].x, 20.0);
        assert_eq!(out[0].y, 40.0);
        assert_eq!(out[0].width, 60.0);
        assert_eq!(out[0].height, 80.0);
    }
}
