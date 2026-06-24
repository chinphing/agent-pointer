use anyhow::{Context, Result};
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;
use lopdf::{Document, xobject::PdfImage};
use std::io::{Cursor, Write};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Max PDF pages processed per `media_understand` call (page-image vision).
pub const MAX_PDF_PAGES_PER_CALL: usize = 10;
/// Default page window when the user does not specify `pageStart` / `pageEnd`.
pub const DEFAULT_PDF_PAGE_END: usize = 10;
/// Legacy alias — same as per-call page cap.
pub const MAX_PDF_OCR_PAGES: usize = MAX_PDF_PAGES_PER_CALL;
const MIN_PDF_IMAGE_DIMENSION: i64 = 64;
const MAX_PDF_IMAGE_BYTES: usize = 6 * 1024 * 1024;
const PDF_RENDER_DPI: u32 = 150;

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW);
        return cmd;
    }
    #[cfg(not(windows))]
    Command::new(program)
}

pub fn pdf_page_count(bytes: &[u8], file_name: &str) -> Result<usize> {
    let doc = Document::load_mem(bytes).with_context(|| format!("load pdf {file_name}"))?;
    Ok(doc.get_pages().len())
}

/// 1-based inclusive page range for PDF extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfPageRange {
    pub start: usize,
    pub end: usize,
    pub user_specified: bool,
}

impl PdfPageRange {
    pub fn page_count(&self) -> usize {
        self.end.saturating_sub(self.start).saturating_add(1)
    }

    pub fn normalize(total_pages: usize, start: usize, end: usize) -> Result<Self> {
        if total_pages == 0 {
            anyhow::bail!("pdf has no pages");
        }
        if start == 0 || end == 0 {
            anyhow::bail!("pageStart and pageEnd are 1-based and must be >= 1");
        }
        if start > end {
            anyhow::bail!("pageStart ({start}) must be <= pageEnd ({end})");
        }
        if end > total_pages {
            anyhow::bail!(
                "pageEnd ({end}) exceeds document page count ({total_pages})"
            );
        }
        Ok(Self {
            start,
            end,
            user_specified: true,
        })
    }

    pub fn default_first_window(total_pages: usize) -> Result<Self> {
        if total_pages == 0 {
            anyhow::bail!("pdf has no pages");
        }
        Ok(Self {
            start: 1,
            end: total_pages.min(DEFAULT_PDF_PAGE_END),
            user_specified: false,
        })
    }

    pub fn ensure_within_per_call_limit(&self) -> Result<()> {
        let count = self.page_count();
        if count > MAX_PDF_PAGES_PER_CALL {
            anyhow::bail!(
                "requested {count} pages ({}-{}); max {MAX_PDF_PAGES_PER_CALL} per call — split into multiple media_understand calls with different pageStart/pageEnd",
                self.start,
                self.end
            );
        }
        Ok(())
    }
}

pub fn format_pdf_scope_notice(range: &PdfPageRange, total_pages: usize) -> String {
    let scope = if range.start == range.end {
        format!("page {}", range.start)
    } else {
        format!("pages {}-{}", range.start, range.end)
    };
    if range.user_specified {
        format!(
            "[PDF scope: {scope} of {total_pages} total pages — extracted as requested.]"
        )
    } else {
        format!(
            "[PDF scope: {scope} of {total_pages} total pages — user did not specify pages; only the first {} pages were processed. Call again with pageStart/pageEnd when they need other pages, or split into batches of at most {MAX_PDF_PAGES_PER_CALL} pages.]"
            ,
            range.end
        )
    }
}

/// Extract page images for vision understanding.
/// Prefers embedded raster images (typical scans); falls back to `pdftoppm` rendering for text PDFs.
pub fn extract_pdf_page_images_base64(bytes: &[u8], file_name: &str) -> Result<Vec<String>> {
    let total = pdf_page_count(bytes, file_name)?;
    let range = PdfPageRange::default_first_window(total)?;
    extract_pdf_page_images_base64_range(bytes, file_name, &range)
}

pub fn extract_pdf_page_images_base64_range(
    bytes: &[u8],
    file_name: &str,
    range: &PdfPageRange,
) -> Result<Vec<String>> {
    let doc = Document::load_mem(bytes).with_context(|| format!("load pdf {file_name}"))?;
    let mut pages: Vec<_> = doc.get_pages().into_iter().collect();
    pages.sort_by_key(|(num, _)| *num);

    let mut frames = Vec::new();
    for (ordinal, (_, page_id)) in pages.into_iter().enumerate() {
        let page_index = ordinal + 1;
        if page_index < range.start || page_index > range.end {
            continue;
        }
        if frames.len() >= MAX_PDF_PAGES_PER_CALL {
            log::info!(
                "pdf {file_name}: reached max OCR pages ({MAX_PDF_PAGES_PER_CALL}) in range {}-{}",
                range.start,
                range.end
            );
            break;
        }
        let images = doc
            .get_page_images(page_id)
            .with_context(|| format!("read page {page_index} images in {file_name}"))?;
        let Some(img) = select_largest_page_image(&images) else {
            continue;
        };
        match pdf_image_to_jpeg_bytes(&img) {
            Ok(jpeg) => {
                if jpeg.is_empty() {
                    log::warn!(
                        "pdf {file_name} page {page_index}: decoded empty jpeg"
                    );
                    continue;
                }
                if jpeg.len() > MAX_PDF_IMAGE_BYTES {
                    log::warn!(
                        "pdf {file_name} page {page_index}: image {} bytes exceeds limit; skipping",
                        jpeg.len()
                    );
                    continue;
                }
                log::info!(
                    "pdf {file_name} page {page_index}: extracted raster {}x{} ({} bytes jpeg)",
                    img.width,
                    img.height,
                    jpeg.len()
                );
                frames.push(base64::engine::general_purpose::STANDARD.encode(jpeg));
            }
            Err(e) => {
                log::warn!(
                    "pdf {file_name} page {page_index}: raster decode failed: {:#}",
                    e
                );
            }
        }
    }

    if !frames.is_empty() {
        return Ok(frames);
    }

    log::info!(
        "pdf {file_name}: no embedded page images in pages {}-{}; rendering with pdftoppm",
        range.start,
        range.end
    );
    render_pdf_pages_base64_with_pdftoppm(bytes, file_name, range)
}

fn render_pdf_pages_base64_with_pdftoppm(
    bytes: &[u8],
    file_name: &str,
    range: &PdfPageRange,
) -> Result<Vec<String>> {
    let pdftoppm = crate::media::ffmpeg::resolve_pdftoppm().ok_or_else(|| {
        anyhow::anyhow!(
            "pdf pages {}-{} in {file_name}: no embedded images and pdftoppm (poppler-utils) not found — install poppler or use the pdf skill with Python",
            range.start,
            range.end
        )
    })?;

    let mut pdf_file = tempfile::Builder::new()
        .suffix(".pdf")
        .tempfile()
        .context("create temp pdf for pdftoppm")?;
    pdf_file
        .write_all(bytes)
        .context("write temp pdf for pdftoppm")?;
    pdf_file
        .flush()
        .context("flush temp pdf for pdftoppm")?;

    let out_dir = tempfile::tempdir().context("temp dir for pdftoppm output")?;
    let prefix = out_dir.path().join("page");
    let prefix_str = prefix
        .to_str()
        .context("pdftoppm output prefix path")?;
    let pdf_path = pdf_file.path().to_str().context("temp pdf path")?;

    let first = range.start;
    let last = range.end.min(first + MAX_PDF_PAGES_PER_CALL - 1);

    let status = hidden_command(&pdftoppm)
        .args([
            "-jpeg",
            "-jpegopt",
            "quality=85",
            "-r",
            &PDF_RENDER_DPI.to_string(),
            "-f",
            &first.to_string(),
            "-l",
            &last.to_string(),
            pdf_path,
            prefix_str,
        ])
        .status()
        .with_context(|| format!("run pdftoppm for {file_name}"))?;

    if !status.success() {
        anyhow::bail!("pdftoppm failed for {file_name} (pages {first}-{last})");
    }

    let mut jpeg_paths: Vec<std::path::PathBuf> = std::fs::read_dir(out_dir.path())
        .with_context(|| format!("read pdftoppm output dir for {file_name}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|ext| ext == "jpg" || ext == "jpeg")
        })
        .collect();
    jpeg_paths.sort();

    let mut frames = Vec::new();
    for jpeg_path in jpeg_paths {
        if frames.len() >= MAX_PDF_PAGES_PER_CALL {
            break;
        }
        let jpeg = std::fs::read(&jpeg_path)
            .with_context(|| format!("read pdftoppm jpeg {:?}", jpeg_path))?;
        if jpeg.is_empty() {
            log::warn!("pdf {file_name}: pdftoppm produced empty jpeg at {:?}", jpeg_path);
            continue;
        }
        if jpeg.len() > MAX_PDF_IMAGE_BYTES {
            log::warn!(
                "pdf {file_name}: rendered image {} bytes exceeds limit; skipping",
                jpeg.len()
            );
            continue;
        }
        log::info!(
            "pdf {file_name}: rendered with pdftoppm ({} bytes jpeg)",
            jpeg.len()
        );
        frames.push(base64::engine::general_purpose::STANDARD.encode(jpeg));
    }

    if frames.is_empty() {
        anyhow::bail!(
            "pdftoppm produced no usable page images for {file_name} (pages {first}-{last})"
        );
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
    fn pdf_scope_notice_default_vs_user() {
        use super::{format_pdf_scope_notice, PdfPageRange};
        let default = PdfPageRange {
            start: 1,
            end: 10,
            user_specified: false,
        };
        let notice = format_pdf_scope_notice(&default, 50);
        assert!(notice.contains("did not specify pages"));
        assert!(notice.contains("1-10"));

        let user = PdfPageRange {
            start: 5,
            end: 7,
            user_specified: true,
        };
        let notice = format_pdf_scope_notice(&user, 50);
        assert!(notice.contains("as requested"));
        assert!(notice.contains("5-7"));
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
