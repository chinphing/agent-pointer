//! Production-shaped pdf path: page count + render in one spawn_blocking (no cross-thread pdfium).
use pointer_core::media::pdf::{extract_pdf_page_images_base64_range, pdf_page_count};
use pointer_core::tools::media_understand::parse_pdf_page_range;
use serde_json::json;
use std::time::Instant;

#[test]
fn production_pdf_path_in_one_spawn_blocking() {
    let Ok(path) = std::env::var("POINTER_PDFIUM_PROBE_PDF") else {
        return;
    };
    if path.trim().is_empty() || !std::path::Path::new(&path).exists() {
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let page_args = json!({});
    let bytes = std::fs::read(&path).unwrap();
    let t0 = Instant::now();
    let result = rt
        .block_on(async {
            tokio::task::spawn_blocking(move || {
                let total = pdf_page_count(&bytes, "probe.pdf")?;
                let range = parse_pdf_page_range(&page_args, total)?;
                let pages = extract_pdf_page_images_base64_range(&bytes, "probe.pdf", &range)?;
                Ok::<_, anyhow::Error>((total, range, pages.len()))
            })
            .await
            .unwrap()
        })
        .unwrap();
    let (total, range, page_frames) = result;
    eprintln!(
        "production path: {total} pages, rendered {} frames, range {}-{}, {}ms",
        page_frames,
        range.start,
        range.end,
        t0.elapsed().as_millis()
    );
    assert!(total > 0);
    assert!(page_frames > 0);
    assert!(t0.elapsed().as_secs() < 15);
}
