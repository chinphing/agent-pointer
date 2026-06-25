//! Resize / re-encode JPEGs before vision-model upload.

use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType, GenericImageView};
use std::io::Cursor;

use super::apply::{HARD_IMAGE_MAX_BYTES, INLINE_IMAGE_MAX_BYTES};

/// Longest side (px) sent to vision models — keeps OCR-readable text while reducing upload size.
pub const VISION_IMAGE_MAX_LONG_EDGE: u32 = 1400;

/// Longest side for web manual desktop preview (matches UI modal max width; 5s polling).
pub const MANUAL_PREVIEW_MAX_LONG_EDGE: u32 = 1280;

/// Target max encoded size per vision image; re-encode / shrink until under this when possible.
pub const VISION_JPEG_MAX_BYTES: usize = INLINE_IMAGE_MAX_BYTES;

/// Target max JPEG for manual desktop preview API (~5s refresh over WAN).
pub const MANUAL_PREVIEW_JPEG_MAX_BYTES: usize = 100 * 1024;

const MIN_LONG_EDGE: u32 = 640;
const MANUAL_PREVIEW_MIN_LONG_EDGE: u32 = 480;
const JPEG_QUALITIES: [u8; 4] = [85, 75, 65, 55];
const MANUAL_PREVIEW_JPEG_QUALITIES: [u8; 4] = [68, 58, 48, 38];

fn downscale_long_edge(img: DynamicImage, max_long_edge: u32, label: &str) -> DynamicImage {
    let (w0, h0) = img.dimensions();
    let long = w0.max(h0);
    if long <= max_long_edge {
        return img;
    }
    let scale = max_long_edge as f32 / long as f32;
    let w1 = ((w0 as f32) * scale).round().max(1.0) as u32;
    let h1 = ((h0 as f32) * scale).round().max(1.0) as u32;
    log::info!(
        "vision jpeg {label}: downscale {w0}x{h0} -> {w1}x{h1} (max_long_edge={max_long_edge})"
    );
    img.resize(w1, h1, image::imageops::FilterType::Triangle)
}

fn encode_jpeg_rgb(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb8 = img.to_rgb8();
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let mut encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
    encoder
        .encode(
            rgb8.as_raw(),
            rgb8.width(),
            rgb8.height(),
            ExtendedColorType::Rgb8,
        )
        .context("encode vision jpeg")?;
    Ok(buf)
}

/// Downscale and re-encode a JPEG for vision upload (PDF pages, large photos, etc.).
pub fn prepare_jpeg_for_vision(bytes: &[u8], label: &str) -> Result<Vec<u8>> {
    prepare_jpeg_with_limits(
        bytes,
        label,
        VISION_IMAGE_MAX_LONG_EDGE,
        VISION_JPEG_MAX_BYTES,
        MIN_LONG_EDGE,
        &JPEG_QUALITIES,
    )
}

/// Downscale and re-encode a desktop JPEG for web manual preview (bandwidth-friendly polling).
pub fn prepare_jpeg_for_manual_preview(bytes: &[u8]) -> Result<Vec<u8>> {
    prepare_jpeg_with_limits(
        bytes,
        "manual-preview",
        MANUAL_PREVIEW_MAX_LONG_EDGE,
        MANUAL_PREVIEW_JPEG_MAX_BYTES,
        MANUAL_PREVIEW_MIN_LONG_EDGE,
        &MANUAL_PREVIEW_JPEG_QUALITIES,
    )
}

fn prepare_jpeg_with_limits(
    bytes: &[u8],
    label: &str,
    max_long_edge: u32,
    max_bytes: usize,
    min_long_edge: u32,
    qualities: &[u8],
) -> Result<Vec<u8>> {
    if bytes.is_empty() {
        anyhow::bail!("vision jpeg {label}: empty input");
    }
    let mut img =
        image::load_from_memory(bytes).with_context(|| format!("decode vision jpeg {label}"))?;
    let (w0, h0) = img.dimensions();
    if bytes.len() <= max_bytes
        && w0.max(h0) <= max_long_edge
    {
        return Ok(bytes.to_vec());
    }

    img = downscale_long_edge(img, max_long_edge, label);

    let mut best = bytes.to_vec();
    if img.dimensions() != (w0, h0) || bytes.len() > max_bytes {
        best.clear();
    }

    'outer: loop {
        for &quality in qualities {
            let encoded = encode_jpeg_rgb(&img, quality)?;
            if best.is_empty() || encoded.len() < best.len() {
                best = encoded;
            }
            if best.len() <= max_bytes {
                break 'outer;
            }
        }

        let (w, h) = img.dimensions();
        let long = w.max(h);
        if long <= min_long_edge {
            break;
        }
        let scale = 0.85_f32;
        let w1 = ((w as f32) * scale).round().max(min_long_edge as f32) as u32;
        let h1 = ((h as f32) * scale).round().max(1.0) as u32;
        log::info!(
            "vision jpeg {label}: still {} bytes > {max_bytes}; shrink {w}x{h} -> {w1}x{h1}",
            best.len()
        );
        img = img.resize(w1, h1, image::imageops::FilterType::Triangle);
        best.clear();
    }

    if best.len() > HARD_IMAGE_MAX_BYTES {
        anyhow::bail!(
            "vision jpeg {label}: {} bytes still exceeds hard limit {HARD_IMAGE_MAX_BYTES}",
            best.len()
        );
    }

    if best.len() < bytes.len() || img.dimensions() != (w0, h0) {
        log::info!(
            "vision jpeg {label}: prepared {} bytes (was {} bytes, {}x{})",
            best.len(),
            bytes.len(),
            w0,
            h0
        );
    }

    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn solid_jpeg(w: u32, h: u32) -> Vec<u8> {
        let rgba = RgbaImage::from_pixel(w, h, image::Rgba([40, 80, 120, 255]));
        let img = DynamicImage::ImageRgba8(rgba);
        encode_jpeg_rgb(&img, 95).unwrap()
    }

    #[test]
    fn downscales_when_long_edge_exceeds_cap() {
        let input = solid_jpeg(1241, 1754);
        let out = prepare_jpeg_for_vision(&input, "test-a4").unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert!(img.width().max(img.height()) <= VISION_IMAGE_MAX_LONG_EDGE);
        assert!(out.len() <= VISION_JPEG_MAX_BYTES);
    }

    #[test]
    fn leaves_small_jpeg_unchanged_when_already_under_limits() {
        let input = solid_jpeg(320, 240);
        let out = prepare_jpeg_for_vision(&input, "test-small").unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(img.dimensions(), (320, 240));
    }

    #[test]
    fn manual_preview_downscales_and_caps_size() {
        let input = solid_jpeg(2560, 1440);
        let out = prepare_jpeg_for_manual_preview(&input).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert!(img.width().max(img.height()) <= MANUAL_PREVIEW_MAX_LONG_EDGE);
        assert!(out.len() <= MANUAL_PREVIEW_JPEG_MAX_BYTES);
    }
}
