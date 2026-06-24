use anyhow::{Context, Result};
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;
use lopdf::{Document, xobject::PdfImage};
use std::io::Cursor;

const MAX_PDF_TEXT_BYTES: usize = 256 * 1024;
/// Max PDF pages processed per `media_understand` call (text or OCR).
pub const MAX_PDF_PAGES_PER_CALL: usize = 10;
/// Default page window when the user does not specify `pageStart` / `pageEnd`.
pub const DEFAULT_PDF_PAGE_END: usize = 10;
/// Below this char count, extracted text is treated as noise (page numbers, watermarks) and OCR fallback runs.
pub const MIN_PDF_TEXT_CHARS: usize = 48;
/// Legacy alias — same as per-call page cap.
pub const MAX_PDF_OCR_PAGES: usize = MAX_PDF_PAGES_PER_CALL;
const MIN_PDF_IMAGE_DIMENSION: i64 = 64;
const MAX_PDF_IMAGE_BYTES: usize = 6 * 1024 * 1024;

pub fn pdf_text_char_count(text: &str) -> usize {
    text.trim().chars().count()
}

pub fn is_pdf_text_sufficient(text: &str) -> bool {
    pdf_text_char_count(text) >= MIN_PDF_TEXT_CHARS
}

/// Heuristic: lopdf reads raw PDF string bytes and often mojibakes CID / Identity-H fonts.
pub fn is_pdf_text_plausible(text: &str) -> bool {
    if !is_pdf_text_sufficient(text) {
        return false;
    }
    pdf_readable_char_ratio(text) >= 0.55
}

fn pdf_readable_char_ratio(text: &str) -> f64 {
    let trimmed = text.trim();
    let total = trimmed.chars().count();
    if total == 0 {
        return 0.0;
    }
    let readable = trimmed
        .chars()
        .filter(|c| is_pdf_readable_char(*c))
        .count();
    readable as f64 / total as f64
}

fn is_pdf_readable_char(c: char) -> bool {
    if c == '\u{FFFD}' {
        return false;
    }
    if c.is_control() && !matches!(c, '\n' | '\r' | '\t') {
        return false;
    }
    if c.is_alphanumeric() {
        return true;
    }
    if ('\u{4e00}'..='\u{9fff}').contains(&c) {
        return true;
    }
    const PUNCT: &str = "，。、；：？！（）《》—…·\"' .,-/\\@#%&*+=[]{}<>:;";
    PUNCT.contains(c)
}

pub fn extract_pdf_text(bytes: &[u8], file_name: &str) -> Result<String> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .with_context(|| format!("pdf extract failed for {file_name}"))?;
    trim_and_validate_pdf_text(&text, file_name)
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

/// Extract PDF text in approximate natural reading order (per-page, Y-desc then X-asc).
pub fn extract_pdf_text_sorted(bytes: &[u8], file_name: &str) -> Result<String> {
    let total = pdf_page_count(bytes, file_name)?;
    let range = PdfPageRange::default_first_window(total)?;
    extract_pdf_text_sorted_range(bytes, file_name, &range)
}

pub fn extract_pdf_text_sorted_range(
    bytes: &[u8],
    file_name: &str,
    range: &PdfPageRange,
) -> Result<String> {
    let doc = Document::load_mem(bytes).with_context(|| format!("load pdf {file_name}"))?;
    let mut pages: Vec<_> = doc.get_pages().into_iter().collect();
    pages.sort_by_key(|(num, _)| *num);

    let mut page_texts: Vec<String> = Vec::new();
    for (ordinal, (page_num, page_id)) in pages.into_iter().enumerate() {
        let page_index = ordinal + 1;
        if page_index < range.start || page_index > range.end {
            continue;
        }
        let content = doc
            .get_and_decode_page_content(page_id)
            .with_context(|| format!("decode page {page_num} in {file_name}"))?;
        let spans = extract_text_spans_from_content(&content);
        if spans.is_empty() {
            continue;
        }
        let sorted = sort_spans_reading_order(spans);
        let joined: String = sorted
            .into_iter()
            .map(|(_, _, s)| s)
            .collect::<Vec<_>>()
            .join(" ");
        let trimmed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
        if !trimmed.is_empty() {
            page_texts.push(format!("--- Page {page_index} ---\n{trimmed}"));
        }
    }

    if page_texts.is_empty() {
        return extract_pdf_text_range(bytes, file_name, range);
    }

    let combined = page_texts.join("\n\n");
    if is_pdf_text_plausible(&combined) {
        return trim_and_validate_pdf_text(&combined, file_name);
    }
    log::info!(
        "pdf {file_name}: lopdf reading-order text failed plausibility (pages {}-{}); using pdf-extract",
        range.start,
        range.end
    );
    extract_pdf_text_range(bytes, file_name, range)
}

fn extract_pdf_text_range(bytes: &[u8], file_name: &str, _range: &PdfPageRange) -> Result<String> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .with_context(|| format!("pdf extract failed for {file_name}"))?;
    // pdf-extract does not expose per-page boundaries reliably; scope notice still applies.
    trim_and_validate_pdf_text(&text, file_name)
}

fn trim_and_validate_pdf_text(text: &str, file_name: &str) -> Result<String> {
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
        Ok(truncate_pdf_text_bytes(trimmed, MAX_PDF_TEXT_BYTES))
    } else {
        Ok(trimmed.to_string())
    }
}

fn truncate_pdf_text_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[derive(Clone)]
struct TextSpan {
    x: f64,
    y: f64,
    text: String,
}

fn object_as_f64(obj: &lopdf::Object) -> Option<f64> {
    obj.as_f32().ok().map(f64::from).or_else(|| obj.as_i64().ok().map(|n| n as f64))
}

fn object_as_string(obj: &lopdf::Object) -> Option<String> {
    obj.as_str()
        .ok()
        .map(|b| String::from_utf8_lossy(b).into_owned())
}

fn extract_text_spans_from_content(content: &lopdf::content::Content) -> Vec<TextSpan> {
    use lopdf::content::Operation;
    let mut spans = Vec::new();
    let mut tx = 0.0f64;
    let mut ty = 0.0f64;
    for op in &content.operations {
        match op {
            Operation { operator, operands } if operator == "Td" || operator == "TD" => {
                if operands.len() >= 2 {
                    if let (Some(x), Some(y)) =
                        (object_as_f64(&operands[0]), object_as_f64(&operands[1]))
                    {
                        tx += x;
                        ty += y;
                    }
                }
            }
            Operation { operator, operands } if operator == "Tm" => {
                if operands.len() >= 6 {
                    if let (Some(x), Some(y)) =
                        (object_as_f64(&operands[4]), object_as_f64(&operands[5]))
                    {
                        tx = x;
                        ty = y;
                    }
                }
            }
            Operation { operator, operands }
                if operator == "Tj" || operator == "'" || operator == "\"" =>
            {
                if let Some(s) = operands.first().and_then(object_as_string) {
                    if !s.is_empty() {
                        spans.push(TextSpan {
                            x: tx,
                            y: ty,
                            text: s,
                        });
                    }
                }
            }
            Operation { operator, operands } if operator == "TJ" => {
                if let Some(arr) = operands.first().and_then(|o| o.as_array().ok()) {
                    for item in arr {
                        if let Some(s) = object_as_string(item) {
                            if !s.is_empty() {
                                spans.push(TextSpan {
                                    x: tx,
                                    y: ty,
                                    text: s,
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    spans
}

fn sort_spans_reading_order(spans: Vec<TextSpan>) -> Vec<(f64, f64, String)> {
    let mut sorted: Vec<(f64, f64, String)> = spans
        .into_iter()
        .map(|s| (s.x, s.y, s.text))
        .collect();
    sorted.sort_by(|a, b| {
        b.1
            .partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    });
    sorted
}


/// Extract embedded page raster images (typical scanned PDFs) and return base64 JPEGs.
/// Pure Rust via `lopdf` + `image`; no system poppler/ghostscript.
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

    if frames.is_empty() {
        anyhow::bail!("pdf contains no decodable embedded page images in pages {}-{}", range.start, range.end);
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
    fn pdf_text_plausibility_rejects_lopdf_cid_garbage() {
        let garbled = format!(
            "--- Page 1 ---\n{}",
            "\u{10}\u{14}\u{FFFD}\u{15}\u{19}ABC\u{7}\u{3}".repeat(20)
        );
        assert!(is_pdf_text_sufficient(&garbled));
        assert!(!is_pdf_text_plausible(&garbled));
        let chinese = "上海市浦东新区人民法院民事调解书原告深圳灯火家园企业管理有限公司被告洪洁茹追偿权纠纷一案本院依法适用小额程序公开开庭进行了审理";
        assert!(is_pdf_text_plausible(chinese));
    }

    #[test]
    #[ignore = "local: Desktop mediation PDF"]
    fn mediation_pdf_extraction_uses_pdf_extract() {
        let path = "/Users/starliu/Desktop/786394_调解书.pdf";
        let bytes = std::fs::read(path).expect("read pdf");
        let file_name = "786394_调解书.pdf";
        let total = pdf_page_count(&bytes, file_name).unwrap();
        let range = PdfPageRange::default_first_window(total).unwrap();
        let text = extract_pdf_text_sorted_range(&bytes, file_name, &range).unwrap();
        assert!(text.contains("民事调解书"));
        assert!(text.contains("上海市浦东新区人民法院"));
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
