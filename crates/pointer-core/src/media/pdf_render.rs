//! Render PDF pages to JPEG via Pdfium (Rust bindings + bundled/cache library).
//! No external CLI tools (poppler/pdftoppm).
//!
//! Must run on a blocking thread when called from async code: first-time Pdfium setup
//! may download the native library through `reqwest::blocking`, which panics on tokio workers.

use crate::media::jpeg_vision::{prepare_dynamic_image_for_vision, VISION_IMAGE_MAX_LONG_EDGE};
use crate::media::pdf::{PdfPageRange, MAX_PDF_IMAGE_BYTES, MAX_PDF_PAGES_PER_CALL, PDF_RENDER_DPI};
use anyhow::{Context, Result};
use base64::Engine as _;
use image::GenericImageView;
use pdfium_auto::bind_pdfium_silent;
use pdfium_render::prelude::*;
use std::cell::RefCell;

thread_local! {
    static PDFIUM: RefCell<Option<Result<Pdfium, String>>> = const { RefCell::new(None) };
}

fn with_pdfium<R>(f: impl FnOnce(&Pdfium) -> Result<R>) -> Result<R> {
    PDFIUM.with(|slot| {
        let mut guard = slot.borrow_mut();
        if guard.is_none() {
            *guard = Some(bind_pdfium_silent().map_err(|e| e.to_string()));
        }
        match guard.as_ref().unwrap() {
            Ok(pdfium) => f(pdfium),
            Err(msg) => Err(anyhow::anyhow!("pdfium unavailable: {msg}")),
        }
    })
}

pub fn pdfium_render_available() -> bool {
    with_pdfium(|_| Ok(())).is_ok()
}

/// Pdfium `scale_page_by_factor` for vision: cap pixel count at [`VISION_IMAGE_MAX_LONG_EDGE`]
/// on the long side; never exceed [`PDF_RENDER_DPI`].
pub fn pdf_render_scale_for_page(long_pts: f32) -> f32 {
    let long_pts = long_pts.max(1.0);
    let scale_dpi = PDF_RENDER_DPI as f32 / 72.0;
    let long_px_at_dpi = long_pts * scale_dpi;
    if long_px_at_dpi <= VISION_IMAGE_MAX_LONG_EDGE as f32 {
        scale_dpi
    } else {
        VISION_IMAGE_MAX_LONG_EDGE as f32 / long_pts
    }
}

/// Render each page in `range` to base64 JPEG for vision models.
pub fn render_pdf_pages_base64_range(
    bytes: &[u8],
    file_name: &str,
    range: &PdfPageRange,
) -> Result<Vec<String>> {
    with_pdfium(|pdfium| render_with_engine(pdfium, bytes, file_name, range))
}

fn render_with_engine(
    pdfium: &Pdfium,
    bytes: &[u8],
    file_name: &str,
    range: &PdfPageRange,
) -> Result<Vec<String>> {
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .with_context(|| format!("pdfium load {file_name}"))?;

    let page_count = document.pages().len() as usize;
    if page_count == 0 {
        anyhow::bail!("pdf {file_name} has no pages");
    }

    let first = range.start;
    let last = range.end.min(first + MAX_PDF_PAGES_PER_CALL - 1);
    let mut frames = Vec::new();

    for page_index in first..=last {
        if frames.len() >= MAX_PDF_PAGES_PER_CALL {
            break;
        }
        let pdfium_index = (page_index - 1) as u16;
        if pdfium_index as usize >= page_count {
            log::warn!(
                "pdf {file_name}: page {page_index} exceeds pdfium page count {page_count}"
            );
            break;
        }
        let page = document
            .pages()
            .get(pdfium_index)
            .with_context(|| format!("pdfium page {page_index} in {file_name}"))?;

        let w_pts = page.width().value;
        let h_pts = page.height().value;
        let long_pts = w_pts.max(h_pts);
        let scale = pdf_render_scale_for_page(long_pts);
        let approx_long_px = (long_pts * scale).round() as u32;

        let config = PdfRenderConfig::new()
            .scale_page_by_factor(scale)
            .render_form_data(true);

        let bitmap = page
            .render_with_config(&config)
            .with_context(|| format!("pdfium render page {page_index} in {file_name}"))?;

        let dynamic = bitmap.as_image();
        let (rw, rh) = dynamic.dimensions();
        let label = format!("{file_name} page {page_index}");
        let jpeg = prepare_dynamic_image_for_vision(dynamic, &label)
            .with_context(|| format!("prepare vision jpeg page {page_index} in {file_name}"))?;

        if jpeg.is_empty() {
            log::warn!("pdf {file_name} page {page_index}: empty jpeg after render");
            continue;
        }
        if jpeg.len() > MAX_PDF_IMAGE_BYTES {
            log::warn!(
                "pdf {file_name} page {page_index}: rendered {} bytes exceeds limit; skipping",
                jpeg.len()
            );
            continue;
        }

        log::info!(
            "pdf {file_name} page {page_index}: pdfium render {rw}x{rh} (scale={scale:.3}, ~{approx_long_px}px long) -> {} bytes jpeg",
            jpeg.len()
        );
        frames.push(base64::engine::general_purpose::STANDARD.encode(&jpeg));
    }

    if frames.is_empty() {
        anyhow::bail!(
            "pdfium produced no usable page images for {file_name} (pages {first}-{last})"
        );
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_render_scale_caps_oversized_scan_pages() {
        let scale = pdf_render_scale_for_page(2480.0);
        assert!((scale - 1400.0 / 2480.0).abs() < 0.001);
        assert!((2480.0 * scale).round() as u32 <= VISION_IMAGE_MAX_LONG_EDGE);
    }

    #[test]
    fn pdf_render_scale_uses_dpi_for_small_pages() {
        let scale = pdf_render_scale_for_page(400.0);
        assert!((scale - PDF_RENDER_DPI as f32 / 72.0).abs() < 0.001);
    }

    #[test]
    fn pdf_render_scale_clamps_a4_to_vision_long_edge() {
        let scale = pdf_render_scale_for_page(841.9);
        let long_px = 841.9 * scale;
        assert!(long_px <= VISION_IMAGE_MAX_LONG_EDGE as f32 + 1.0);
    }

    #[test]
    #[ignore = "requires pdfium download/cache"]
    fn pdfium_renders_local_pdf_when_available() {
        if !pdfium_render_available() {
            return;
        }
        let path = "/Users/starliu/Desktop/786394_调解书.pdf";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let bytes = std::fs::read(path).expect("read pdf");
        let range = PdfPageRange {
            start: 1,
            end: 1,
            user_specified: true,
        };
        let frames = render_pdf_pages_base64_range(&bytes, "调解书.pdf", &range).unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].len() > 1000);
    }

    #[test]
    #[ignore = "requires pdfium download/cache"]
    fn pdfium_renders_oversized_scan_pdf_when_available() {
        if !pdfium_render_available() {
            return;
        }
        let path = "/Users/starliu/Desktop/案件材料/证据8：放款凭证.pdf";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let bytes = std::fs::read(path).expect("read pdf");
        let range = PdfPageRange {
            start: 1,
            end: 1,
            user_specified: true,
        };
        let t0 = std::time::Instant::now();
        let frames = render_pdf_pages_base64_range(&bytes, "证据8：放款凭证.pdf", &range).unwrap();
        eprintln!("scan pdf render elapsed: {}ms", t0.elapsed().as_millis());
        assert_eq!(frames.len(), 1);
        let decoded = base64::engine::general_purpose::STANDARD.decode(&frames[0]).unwrap();
        let img = image::load_from_memory(&decoded).unwrap();
        assert!(img.width().max(img.height()) <= VISION_IMAGE_MAX_LONG_EDGE);
        assert!(decoded.len() <= crate::media::jpeg_vision::VISION_JPEG_MAX_BYTES);
    }
}
