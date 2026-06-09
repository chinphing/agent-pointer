use anyhow::{Context, Result};
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;
use lopdf::{Document, xobject::PdfImage};
use std::io::Cursor;

const MAX_PDF_TEXT_BYTES: usize = 256 * 1024;
/// Below this char count, extracted text is treated as noise (page numbers, watermarks) and OCR fallback runs.
pub const MIN_PDF_TEXT_CHARS: usize = 48;
/// Max PDF pages sent to the image understanding model (scanned / image-only PDFs).
pub const MAX_PDF_OCR_PAGES: usize = 10;
const MIN_PDF_IMAGE_DIMENSION: i64 = 64;
const MAX_PDF_IMAGE_BYTES: usize = 6 * 1024 * 1024;

pub fn pdf_text_char_count(text: &str) -> usize {
    text.trim().chars().count()
}

pub fn is_pdf_text_sufficient(text: &str) -> bool {
    pdf_text_char_count(text) >= MIN_PDF_TEXT_CHARS
}

pub fn extract_pdf_text(bytes: &[u8], file_name: &str) -> Result<String> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .with_context(|| format!("pdf extract failed for {file_name}"))?;
    let trimmed = text.trim();
    let char_count = trimmed.chars().count();
    if char_count < MIN_PDF_TEXT_CHARS {
        anyhow::bail!(
            "pdf text below threshold: {char_count} chars (min {MIN_PDF_TEXT_CHARS})"
        );
    }
    if trimmed.len() > MAX_PDF_TEXT_BYTES {
        log::warn!(
            "pdf {file_name} text exceeds {} bytes; truncating",
            MAX_PDF_TEXT_BYTES
        );
        Ok(trimmed[..MAX_PDF_TEXT_BYTES].to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

/// Extract embedded page raster images (typical scanned PDFs) and return base64 JPEGs.
/// Pure Rust via `lopdf` + `image`; no system poppler/ghostscript.
pub fn extract_pdf_page_images_base64(bytes: &[u8], file_name: &str) -> Result<Vec<String>> {
    let doc = Document::load_mem(bytes).with_context(|| format!("load pdf {file_name}"))?;
    let mut pages: Vec<_> = doc.get_pages().into_iter().collect();
    pages.sort_by_key(|(num, _)| *num);

    let mut frames = Vec::new();
    for (page_idx, (_, page_id)) in pages.into_iter().enumerate() {
        if frames.len() >= MAX_PDF_OCR_PAGES {
            log::info!(
                "pdf {file_name}: reached max OCR pages ({MAX_PDF_OCR_PAGES}); skipping remaining pages"
            );
            break;
        }
        let images = doc
            .get_page_images(page_id)
            .with_context(|| format!("read page {} images in {file_name}", page_idx + 1))?;
        let Some(img) = select_largest_page_image(&images) else {
            continue;
        };
        match pdf_image_to_jpeg_bytes(&img) {
            Ok(jpeg) => {
                if jpeg.is_empty() {
                    log::warn!(
                        "pdf {file_name} page {}: decoded empty jpeg",
                        page_idx + 1
                    );
                    continue;
                }
                if jpeg.len() > MAX_PDF_IMAGE_BYTES {
                    log::warn!(
                        "pdf {file_name} page {}: image {} bytes exceeds limit; skipping",
                        page_idx + 1,
                        jpeg.len()
                    );
                    continue;
                }
                log::info!(
                    "pdf {file_name} page {}: extracted raster {}x{} ({} bytes jpeg)",
                    page_idx + 1,
                    img.width,
                    img.height,
                    jpeg.len()
                );
                frames.push(base64::engine::general_purpose::STANDARD.encode(jpeg));
            }
            Err(e) => {
                log::warn!(
                    "pdf {file_name} page {}: raster decode failed: {:#}",
                    page_idx + 1,
                    e
                );
            }
        }
    }

    if frames.is_empty() {
        anyhow::bail!("pdf contains no decodable embedded page images");
    }
    Ok(frames)
}

fn select_largest_page_image<'a>(images: &'a [PdfImage<'a>]) -> Option<&'a PdfImage<'a>> {
    images
        .iter()
        .filter(|img| img.width >= MIN_PDF_IMAGE_DIMENSION && img.height >= MIN_PDF_IMAGE_DIMENSION)
        .max_by_key(|img| img.width.saturating_mul(img.height))
}

fn pdf_image_to_jpeg_bytes(img: &PdfImage<'_>) -> Result<Vec<u8>> {
    let filters = img.filters.as_deref().unwrap_or(&[]);
    let mut data = img.content.to_vec();

    for filter in filters {
        data = match filter.as_str() {
            "FlateDecode" => flate_decode(&data)?,
            "DCTDecode" => return normalize_to_jpeg(&data),
            "ASCII85Decode" => ascii85_decode(&data)?,
            "ASCIIHexDecode" => ascii_hex_decode(&data)?,
            "JPXDecode" => anyhow::bail!("JPXDecode (JPEG2000) is not supported"),
            "LZWDecode" => anyhow::bail!("LZWDecode image stream is not supported"),
            other => {
                log::warn!("pdf image: unsupported filter {other}");
                data
            }
        };
    }

    if looks_like_jpeg(&data) {
        return Ok(data);
    }
    if looks_like_png(&data) {
        return png_bytes_to_jpeg(&data);
    }
    raster_bytes_to_jpeg(img, &data)
}

fn normalize_to_jpeg(data: &[u8]) -> Result<Vec<u8>> {
    if looks_like_jpeg(data) {
        return Ok(data.to_vec());
    }
    png_bytes_to_jpeg(data)
}

fn looks_like_jpeg(data: &[u8]) -> bool {
    data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF
}

fn looks_like_png(data: &[u8]) -> bool {
    data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
}

fn flate_decode(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(data.len().saturating_mul(2));
    decoder
        .read_to_end(&mut out)
        .context("flate decode pdf image")?;
    Ok(out)
}

fn ascii85_decode(input: &[u8]) -> Result<Vec<u8>> {
    let trimmed: Vec<u8> = input
        .iter()
        .copied()
        .filter(|&b| !b.is_ascii_whitespace())
        .collect();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut tuple: u32 = 0;
    let mut count = 0usize;
    for &b in &trimmed {
        if b == b'z' {
            if count != 0 {
                anyhow::bail!("invalid ASCII85: z inside group");
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if b == b'~' {
            break;
        }
        if !(b'!'..=b'u').contains(&b) {
            continue;
        }
        tuple = tuple * 85 + (b - b'!') as u32;
        count += 1;
        if count == 5 {
            out.extend_from_slice(&tuple.to_be_bytes());
            tuple = 0;
            count = 0;
        }
    }
    if count > 0 {
        for _ in count..5 {
            tuple = tuple * 85 + 84;
        }
        let bytes = tuple.to_be_bytes();
        out.extend_from_slice(&bytes[..count.saturating_sub(1)]);
    }
    Ok(out)
}

fn ascii_hex_decode(input: &[u8]) -> Result<Vec<u8>> {
    let hex: String = input
        .iter()
        .copied()
        .filter(|b| b.is_ascii_hexdigit())
        .map(|b| b as char)
        .collect();
    if hex.is_empty() {
        return Ok(Vec::new());
    }
    let padded = if hex.len() % 2 == 1 {
        format!("{hex}0")
    } else {
        hex
    };
    let mut out = Vec::with_capacity(padded.len() / 2);
    let chars: Vec<char> = padded.chars().collect();
    for chunk in chars.chunks(2) {
        let pair: String = chunk.iter().collect();
        let byte = u8::from_str_radix(&pair, 16).context("ascii hex decode")?;
        out.push(byte);
    }
    Ok(out)
}

fn png_bytes_to_jpeg(data: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(data).context("decode png image from pdf")?;
    encode_dynamic_image_jpeg(&img)
}

fn raster_bytes_to_jpeg(img: &PdfImage<'_>, data: &[u8]) -> Result<Vec<u8>> {
    let width = usize::try_from(img.width.max(0)).context("pdf image width")?;
    let height = usize::try_from(img.height.max(0)).context("pdf image height")?;
    if width == 0 || height == 0 {
        anyhow::bail!("pdf image has invalid dimensions");
    }
    let bpc = usize::try_from(img.bits_per_component.unwrap_or(8).max(1)).context("pdf bpc")?;
    if bpc != 8 {
        anyhow::bail!("pdf image bits_per_component={bpc} is not supported");
    }

    let color_space = img
        .color_space
        .as_deref()
        .unwrap_or("DeviceRGB")
        .to_ascii_lowercase();
    let components = match color_space.as_str() {
        "devicegray" | "calgray" => 1usize,
        "devicergb" | "calrgb" => 3usize,
        "devicecmyk" => 4usize,
        _ => {
            log::warn!("pdf image: unknown ColorSpace {color_space}; assuming DeviceRGB");
            3usize
        }
    };

    let expected = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(components))
        .context("pdf image byte size overflow")?;
    if data.len() < expected {
        anyhow::bail!(
            "pdf image raster too short: got {} expected >= {expected}",
            data.len()
        );
    }

    let rgb = match components {
        1 => {
            let mut out = Vec::with_capacity(width * height * 3);
            for &g in &data[..expected] {
                out.extend_from_slice(&[g, g, g]);
            }
            out
        }
        3 => data[..expected].to_vec(),
        4 => cmyk_to_rgb(&data[..expected]),
        _ => anyhow::bail!("unsupported pdf image components={components}"),
    };

    encode_rgb_jpeg(&rgb, width as u32, height as u32)
}

fn cmyk_to_rgb(cmyk: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((cmyk.len() / 4) * 3);
    for chunk in cmyk.chunks_exact(4) {
        let c = chunk[0] as f32 / 255.0;
        let m = chunk[1] as f32 / 255.0;
        let y = chunk[2] as f32 / 255.0;
        let k = chunk[3] as f32 / 255.0;
        let r = 255.0 * (1.0 - c) * (1.0 - k);
        let g = 255.0 * (1.0 - m) * (1.0 - k);
        let b = 255.0 * (1.0 - y) * (1.0 - k);
        rgb.push(r.round().clamp(0.0, 255.0) as u8);
        rgb.push(g.round().clamp(0.0, 255.0) as u8);
        rgb.push(b.round().clamp(0.0, 255.0) as u8);
    }
    rgb
}

fn encode_dynamic_image_jpeg(img: &image::DynamicImage) -> Result<Vec<u8>> {
    let rgb8 = img.to_rgb8();
    encode_rgb_jpeg(rgb8.as_raw(), rgb8.width(), rgb8.height())
}

fn encode_rgb_jpeg(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let mut encoder = JpegEncoder::new_with_quality(&mut cursor, 85);
    encoder
        .encode(rgb, width, height, ExtendedColorType::Rgb8)
        .context("encode pdf raster as jpeg")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_text_threshold() {
        assert!(!is_pdf_text_sufficient(""));
        assert!(!is_pdf_text_sufficient("1\n2\n3\npage 4"));
        assert!(is_pdf_text_sufficient(&"a".repeat(MIN_PDF_TEXT_CHARS)));
    }

    #[test]
    fn jpeg_magic_detected() {
        assert!(looks_like_jpeg(&[0xFF, 0xD8, 0xFF, 0x00]));
        assert!(!looks_like_jpeg(&[0x89, 0x50]));
    }

    #[test]
    fn raster_gray_to_jpeg() {
        let w = 2u32;
        let h = 2u32;
        let data = vec![0u8, 128, 200, 255];
        let img = PdfImage {
            id: (1, 0),
            width: w as i64,
            height: h as i64,
            color_space: Some("DeviceGray".into()),
            filters: None,
            bits_per_component: Some(8),
            content: &data,
            origin_dict: &lopdf::Dictionary::new(),
        };
        let jpeg = raster_bytes_to_jpeg(&img, &data).unwrap();
        assert!(looks_like_jpeg(&jpeg));
    }
}
