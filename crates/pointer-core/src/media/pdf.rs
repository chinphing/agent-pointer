use anyhow::{Context, Result};

const MAX_PDF_TEXT_BYTES: usize = 256 * 1024;

pub fn extract_pdf_text(bytes: &[u8], file_name: &str) -> Result<String> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .with_context(|| format!("pdf extract failed for {file_name}"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("pdf contains no extractable text");
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
