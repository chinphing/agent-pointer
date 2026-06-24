//! Render PDF pages to JPEG via Pdfium (Rust bindings + bundled/cache library).
//! No external CLI tools (poppler/pdftoppm).
//!
//! Must run on a blocking thread when called from async code: first-time Pdfium setup
//! may download the native library through `reqwest::blocking`, which panics on tokio workers.

use crate::media::jpeg_vision::prepare_jpeg_for_vision;
use crate::media::pdf::{PdfPageRange, MAX_PDF_IMAGE_BYTES, MAX_PDF_PAGES_PER_CALL, PDF_RENDER_DPI};
use anyhow::{Context, Result};
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
    let scale = PDF_RENDER_DPI as f32 / 72.0;
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

        let config = PdfRenderConfig::new()
            .scale_page_by_factor(scale)
            .render_form_data(true);

        let bitmap = page
            .render_with_config(&config)
            .with_context(|| format!("pdfium render page {page_index} in {file_name}"))?;

        let dynamic = bitmap.as_image();
        let jpeg = encode_dynamic_image_jpeg(&dynamic)
            .with_context(|| format!("encode jpeg page {page_index} in {file_name}"))?;
        let jpeg = prepare_jpeg_for_vision(&jpeg, &format!("{file_name} page {page_index}"))
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
            "pdf {file_name} page {page_index}: pdfium render ({} bytes jpeg)",
            jpeg.len()
        );
        frames.push(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &jpeg,
        ));
    }

    if frames.is_empty() {
        anyhow::bail!(
            "pdfium produced no usable page images for {file_name} (pages {first}-{last})"
        );
    }
    Ok(frames)
}

fn encode_dynamic_image_jpeg(img: &image::DynamicImage) -> Result<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ExtendedColorType;
    use std::io::Cursor;

    let rgb8 = img.to_rgb8();
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let mut encoder = JpegEncoder::new_with_quality(&mut cursor, 85);
    encoder
        .encode(
            rgb8.as_raw(),
            rgb8.width(),
            rgb8.height(),
            ExtendedColorType::Rgb8,
        )
        .context("encode pdfium page as jpeg")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::pdf::PdfPageRange;

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
}
