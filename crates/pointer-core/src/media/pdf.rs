use anyhow::Result;

pub(crate) const PDF_RENDER_DPI: u32 = 150;
pub(crate) const MAX_PDF_IMAGE_BYTES: usize = 6 * 1024 * 1024;

/// Max PDF pages processed per `media_understand` call (page-image vision).
pub const MAX_PDF_PAGES_PER_CALL: usize = 10;
/// Default page window when the user does not specify `pageStart` / `pageEnd`.
pub const DEFAULT_PDF_PAGE_END: usize = 10;
/// Legacy alias — same as per-call page cap.
pub const MAX_PDF_OCR_PAGES: usize = MAX_PDF_PAGES_PER_CALL;

/// Page count for PDF vision pipeline (Pdfium; same engine as page rendering).
/// Call only from the same blocking thread that renders — not from tokio workers while
/// another thread is in pdfium.
pub fn pdf_page_count(bytes: &[u8], file_name: &str) -> Result<usize> {
    let count = super::pdf_render::pdfium_page_count(bytes, file_name)?;
    if count == 0 {
        anyhow::bail!("pdf {file_name} has no pages");
    }
    Ok(count)
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
            anyhow::bail!("pageEnd ({end}) exceeds document page count ({total_pages})");
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
        format!("[PDF scope: {scope} of {total_pages} total pages — extracted as requested.]")
    } else {
        format!(
            "[PDF scope: {scope} of {total_pages} total pages — user did not specify pages; only the first {} pages were processed. Call again with pageStart/pageEnd when they need other pages, or split into batches of at most {MAX_PDF_PAGES_PER_CALL} pages.]"
            ,
            range.end
        )
    }
}

/// Render each page in `range` to JPEG (Pdfium) for vision understanding.
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
    log::info!(
        "pdf {file_name}: rendering pages {}-{} to images via pdfium",
        range.start,
        range.end
    );
    super::pdf_render::render_pdf_pages_base64_range(bytes, file_name, range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires pdfium"]
    fn pdf_page_count_uses_pdfium_for_concatenated_scan_pdf() {
        use crate::media::pdf_render::pdfium_render_available;

        if !pdfium_render_available() {
            return;
        }
        let path = "/Users/starliu/.pointer/skills/weilin-case-query/output/case_908005_doc.pdf";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let bytes = std::fs::read(path).expect("read pdf");
        assert_eq!(pdf_page_count(&bytes, "case_908005_doc.pdf").unwrap(), 3);
    }

    #[test]
    fn pdf_scope_notice_default_vs_user() {
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
}
