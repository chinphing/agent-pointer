use crate::platform_auth::SharedPlatformAuth;
use reqwest::multipart;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
use std::time::{Duration, Instant};

use super::screen;
use crate::platform_endpoints;
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

/// Errors from the annotation HTTP client (§3.2.2).
#[derive(Debug)]
pub enum AnnotateError {
    /// Transport / connection failure.
    Network(String),
    /// Non-success HTTP status from the annotation service.
    ServiceError { status: u16, body: String },
    /// Response body could not be interpreted (JSON, base64, geometry).
    InvalidResponse(String),
    /// Input image could not be decoded or re-encoded for upload.
    ImageEncode(String),
    /// Failed to build the HTTP client.
    ClientBuild(String),
}

impl fmt::Display for AnnotateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnnotateError::Network(s) => write!(f, "annotation network error: {s}"),
            AnnotateError::ServiceError { status, body } => {
                write!(f, "annotation service error (HTTP {status}): {body}")
            }
            AnnotateError::InvalidResponse(s) => write!(f, "annotation invalid response: {s}"),
            AnnotateError::ImageEncode(s) => write!(f, "annotation image encode: {s}"),
            AnnotateError::ClientBuild(s) => write!(f, "annotation client: {s}"),
        }
    }
}

impl std::error::Error for AnnotateError {}

/// Request body contract for `POST /api/v1/annotate/all` (§3.2.2 appendix).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotateRequest {
    /// Raw image bytes (any format supported by [`image::load_from_memory`]; typically JPEG or PNG).
    pub image: Vec<u8>,
    pub threshold: f32,
    pub overlap_threshold: f32,
    pub padding: i32,
}

impl AnnotateRequest {
    /// Default thresholds matching the live annotation service contract.
    pub fn with_image(image: Vec<u8>) -> Self {
        Self {
            image,
            threshold: DEFAULT_THRESHOLD,
            overlap_threshold: DEFAULT_OVERLAP_THRESHOLD,
            padding: DEFAULT_PADDING,
        }
    }
}

/// Successful annotation result: overlay image + boxes in **capture / bitmap** space.
#[derive(Debug, Clone)]
pub struct AnnotateResponse {
    pub image: Vec<u8>,
    pub boxes: Vec<BoxInfo>,
}

/// Map from overlay index to box geometry in bitmap space (§3.2.2).
pub type IndexMap = HashMap<u32, BoxInfo>;

/// Build an [`IndexMap`] from a box list (one entry per index).
pub fn boxes_to_index_map(boxes: &[BoxInfo]) -> IndexMap {
    boxes.iter().map(|b| (b.index, b.clone())).collect()
}

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
    platform_auth: Option<SharedPlatformAuth>,
}

impl AnnotateClient {
    /// Create a new AnnotateClient with default settings.
    pub fn new() -> Result<Self, AnnotateError> {
        Self::with_base_url(&platform_endpoints::annotate_api_base())
    }

    /// Create a new AnnotateClient with a custom base URL.
    ///
    /// # Arguments
    /// * `base_url` - The base URL of the annotation service.
    pub fn with_base_url(base_url: &str) -> Result<Self, AnnotateError> {
        Self::with_base_url_and_auth(base_url, None)
    }

    pub fn with_base_url_and_auth(
        base_url: &str,
        platform_auth: Option<SharedPlatformAuth>,
    ) -> Result<Self, AnnotateError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()
            .map_err(|e| AnnotateError::ClientBuild(e.to_string()))?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            platform_auth,
        })
    }

    /// Create a new AnnotateClient from environment variables.
    /// Uses `COMPUTER_ANNOTATE_API_BASE` or built-in default (see `platform_endpoints`).
    pub fn from_env() -> Result<Self, AnnotateError> {
        Self::with_base_url(&platform_endpoints::annotate_api_base())
    }

    /// Annotate using an explicit [`AnnotateRequest`] (thresholds + image).
    pub async fn annotate(&self, req: &AnnotateRequest) -> Result<AnnotateResponse, AnnotateError> {
        self.annotate_inner(
            &req.image,
            req.threshold,
            req.overlap_threshold,
            req.padding,
        )
        .await
    }

    /// Send an image to the annotation service and get back annotated results (default thresholds).
    ///
    /// # Arguments
    /// * `image_bytes` - Raw screenshot bytes (e.g. JPEG or PNG).
    pub async fn annotate_image(&self, image_bytes: &[u8]) -> Result<AnnotateResponse, AnnotateError> {
        self.annotate_inner(
            image_bytes,
            DEFAULT_THRESHOLD,
            DEFAULT_OVERLAP_THRESHOLD,
            DEFAULT_PADDING,
        )
        .await
    }

    async fn annotate_inner(
        &self,
        image_bytes: &[u8],
        threshold: f32,
        overlap_threshold: f32,
        padding: i32,
    ) -> Result<AnnotateResponse, AnnotateError> {
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
        let upload_png_len = png_bytes.len();
        let prepare_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let file_part = multipart::Part::bytes(png_bytes)
            .file_name("screen.png")
            .mime_str("image/png")
            .map_err(|e| AnnotateError::InvalidResponse(e.to_string()))?;
        let form = multipart::Form::new()
            .part("file", file_part)
            .text("threshold", format!("{threshold}"))
            .text("overlap_threshold", format!("{overlap_threshold}"))
            .text("padding", format!("{padding}"));
        let form_ms = t.elapsed().as_secs_f64() * 1000.0;

        let url = format!("{}/api/v1/annotate/all", self.base_url);
        let t = Instant::now();
        let mut req = self.client.post(&url).multipart(form);
        if let Some(auth) = &self.platform_auth {
            match auth.ensure_access_token().await {
                Ok(token) => {
                    req = req.header("Authorization", format!("Bearer {token}"));
                }
                Err(e) => {
                    return Err(AnnotateError::Network(format!(
                        "platform login required: {e}"
                    )));
                }
            }
        }
        let response = req
            .send()
            .await
            .map_err(|e| AnnotateError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AnnotateError::ServiceError { status, body: text });
        }

        let annotate_response: AnnotateAllResponse = response
            .json()
            .await
            .map_err(|e| AnnotateError::InvalidResponse(e.to_string()))?;
        let http_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let boxes_api = boxes_xyxy_to_box_infos(&annotate_response.boxes)?;
        let out_image =
            decode_base64_image(strip_data_url_prefix(&annotate_response.image_base64))?;
        let boxes = scale_boxes_to_capture_space(boxes_api, scale_x, scale_y);
        let decode_ms = t.elapsed().as_secs_f64() * 1000.0;

        log::info!(
            "annotate_image: prepare_downscale {:.1}ms, build_multipart {:.1}ms, post+parse_json {:.1}ms, decode_boxes_png {:.1}ms, total {:.1}ms ({} boxes, max_edge={}, input={}, upload_png={}, output_png={})",
            prepare_ms,
            form_ms,
            http_ms,
            decode_ms,
            t_total.elapsed().as_secs_f64() * 1000.0,
            boxes.len(),
            max_edge,
            screen::format_data_size_bytes(image_bytes.len()),
            screen::format_data_size_bytes(upload_png_len),
            screen::format_data_size_bytes(out_image.len())
        );

        Ok(AnnotateResponse {
            image: out_image,
            boxes,
        })
    }
}

impl Default for AnnotateClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default AnnotateClient")
    }
}

fn boxes_xyxy_to_box_infos(rows: &[Vec<f64>]) -> Result<Vec<BoxInfo>, AnnotateError> {
    let mut out = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        if row.len() != 4 {
            return Err(AnnotateError::InvalidResponse(format!(
                "annotate boxes: expected [x1,y1,x2,y2], got len {}: {:?}",
                row.len(),
                row
            )));
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
fn prepare_png_for_annotation_upload(bytes: &[u8], max_long_edge: u32) -> Result<PreparedUpload, AnnotateError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AnnotateError::ImageEncode(format!("decode screenshot for annotate: {e}")))?;
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
            .map_err(|e| AnnotateError::ImageEncode(format!("encode PNG for annotate: {e}")))?;
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
fn decode_base64_image(encoded: &str) -> Result<Vec<u8>, AnnotateError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(encoded)
        .map_err(|e| AnnotateError::InvalidResponse(format!("decode base64 image: {e}")))
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
        assert_eq!(
            client.base_url.trim_end_matches('/'),
            platform_endpoints::DEFAULT_ANNOTATE_API_BASE.trim_end_matches('/')
        );
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
    fn boxes_to_index_map_matches_indices() {
        let boxes = vec![
            BoxInfo {
                index: 1,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                confidence: 1.0,
            },
            BoxInfo {
                index: 2,
                x: 5.0,
                y: 5.0,
                width: 3.0,
                height: 3.0,
                confidence: 1.0,
            },
        ];
        let m = boxes_to_index_map(&boxes);
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&1).unwrap().width, 10.0);
        assert_eq!(m.get(&2).unwrap().x, 5.0);
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
