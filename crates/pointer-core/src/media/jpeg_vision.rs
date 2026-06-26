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
/// Desktop screenshot encode ladder (matches [`crate::agents::computer::vision::screen::SCREENSHOT_JPEG_QUALITY`]).
const SCREENSHOT_JPEG_QUALITIES: [u8; 5] = [88, 85, 75, 65, 55];
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

/// Downscale and encode a decoded image for vision upload (PDF pages, etc.).
/// Avoids an intermediate JPEG encode/decode when the source is already a bitmap.
pub fn prepare_dynamic_image_for_vision(img: DynamicImage, label: &str) -> Result<Vec<u8>> {
    prepare_image_with_limits(
        img,
        label,
        None,
        VISION_IMAGE_MAX_LONG_EDGE,
        VISION_JPEG_MAX_BYTES,
        MIN_LONG_EDGE,
        &JPEG_QUALITIES,
    )
}

/// Encode a desktop screenshot at full logical resolution.
/// Reuses the vision JPEG byte cap ([`VISION_JPEG_MAX_BYTES`]) but only lowers quality — no downscale.
pub fn prepare_screenshot_jpeg(img: DynamicImage, label: &str) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let long = w.max(h).max(1);
    let out = prepare_image_with_limits(
        img,
        label,
        None,
        long,
        VISION_JPEG_MAX_BYTES,
        long,
        &SCREENSHOT_JPEG_QUALITIES,
    )?;
    if out.len() > VISION_JPEG_MAX_BYTES {
        log::warn!(
            "vision jpeg {label}: {} bytes still exceeds target {VISION_JPEG_MAX_BYTES} at full {}x{} resolution",
            out.len(),
            w,
            h
        );
    }
    Ok(out)
}

/// Downscale and re-encode a JPEG for vision upload (photos, attachments, etc.).
pub fn prepare_jpeg_for_vision(bytes: &[u8], label: &str) -> Result<Vec<u8>> {
    if bytes.is_empty() {
        anyhow::bail!("vision jpeg {label}: empty input");
    }
    let img =
        image::load_from_memory(bytes).with_context(|| format!("decode vision jpeg {label}"))?;
    let (w0, h0) = img.dimensions();
    if bytes.len() <= VISION_JPEG_MAX_BYTES && w0.max(h0) <= VISION_IMAGE_MAX_LONG_EDGE {
        return Ok(bytes.to_vec());
    }
    prepare_image_with_limits(
        img,
        label,
        Some((bytes.len(), (w0, h0))),
        VISION_IMAGE_MAX_LONG_EDGE,
        VISION_JPEG_MAX_BYTES,
        MIN_LONG_EDGE,
        &JPEG_QUALITIES,
    )
}

/// Downscale and re-encode a desktop JPEG for web manual preview (bandwidth-friendly polling).
pub fn prepare_jpeg_for_manual_preview(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.is_empty() {
        anyhow::bail!("vision jpeg manual-preview: empty input");
    }
    let img = image::load_from_memory(bytes).context("decode manual preview jpeg")?;
    let (w0, h0) = img.dimensions();
    if bytes.len() <= MANUAL_PREVIEW_JPEG_MAX_BYTES && w0.max(h0) <= MANUAL_PREVIEW_MAX_LONG_EDGE {
        return Ok(bytes.to_vec());
    }
    prepare_image_with_limits(
        img,
        "manual-preview",
        Some((bytes.len(), (w0, h0))),
        MANUAL_PREVIEW_MAX_LONG_EDGE,
        MANUAL_PREVIEW_JPEG_MAX_BYTES,
        MANUAL_PREVIEW_MIN_LONG_EDGE,
        &MANUAL_PREVIEW_JPEG_QUALITIES,
    )
}

fn prepare_image_with_limits(
    mut img: DynamicImage,
    label: &str,
    source: Option<(usize, (u32, u32))>,
    max_long_edge: u32,
    max_bytes: usize,
    min_long_edge: u32,
    qualities: &[u8],
) -> Result<Vec<u8>> {
    let (w0, h0) = img.dimensions();
    img = downscale_long_edge(img, max_long_edge, label);

    let mut best = Vec::new();

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

    let changed_dims = img.dimensions() != (w0, h0);
    if let Some((was_bytes, (sw, sh))) = source {
        if best.len() < was_bytes || changed_dims {
            log::info!(
                "vision jpeg {label}: prepared {} bytes (was {} bytes, {sw}x{sh})",
                best.len(),
                was_bytes
            );
        }
    } else if changed_dims {
        log::info!(
            "vision jpeg {label}: prepared {} bytes (from {w0}x{h0})",
            best.len()
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
    fn screenshot_jpeg_preserves_dimensions() {
        let rgba = RgbaImage::from_pixel(1920, 1080, image::Rgba([40, 80, 120, 255]));
        let img = DynamicImage::ImageRgba8(rgba);
        let out = prepare_screenshot_jpeg(img, "test-screenshot").unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!(decoded.dimensions(), (1920, 1080));
        assert!(out.len() <= VISION_JPEG_MAX_BYTES);
    }

    #[test]
    fn dynamic_image_skips_jpeg_roundtrip() {
        let rgba = RgbaImage::from_pixel(2000, 1500, image::Rgba([40, 80, 120, 255]));
        let img = DynamicImage::ImageRgba8(rgba);
        let out = prepare_dynamic_image_for_vision(img, "test-dynamic").unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert!(decoded.width().max(decoded.height()) <= VISION_IMAGE_MAX_LONG_EDGE);
        assert!(out.len() <= VISION_JPEG_MAX_BYTES);
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
