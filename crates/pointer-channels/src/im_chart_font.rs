//! Load a system CJK font for IM chart PNG rasterization.
//!
//! Avoids linking fulgur-chart's bundled Noto Sans JP (~4.3MB) by passing
//! runtime font bytes into `render_chart_to_png`. Fulgur always uses face
//! index 0, so candidates must expose CJK glyphs on that face.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};

/// Resolved system font used for IM chart labels.
pub struct ImChartFont {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    /// Stable identity for PNG cache keys (path + size + mtime).
    pub identity: String,
}

static FONT: OnceLock<Result<ImChartFont, String>> = OnceLock::new();

/// Return the cached system CJK font, loading on first use.
pub fn im_chart_font() -> Result<&'static ImChartFont> {
    match FONT.get_or_init(|| try_load_im_chart_font().map_err(|e| format!("{e:#}"))) {
        Ok(font) => Ok(font),
        Err(msg) => Err(anyhow!("im chart font: {msg}")),
    }
}

fn try_load_im_chart_font() -> Result<ImChartFont> {
    if let Ok(override_path) = std::env::var("POINTER_IM_CHART_FONT") {
        let trimmed = override_path.trim();
        if !trimmed.is_empty() {
            return load_font_file(Path::new(trimmed))
                .with_context(|| format!("POINTER_IM_CHART_FONT={trimmed}"));
        }
    }

    let mut last_err: Option<anyhow::Error> = None;
    for path in system_font_candidates() {
        match load_font_file(&path) {
            Ok(font) => {
                log::info!(
                    "im_chart_font: using {} ({} bytes)",
                    font.path.display(),
                    font.bytes.len()
                );
                return Ok(font);
            }
            Err(err) => {
                log::debug!("im_chart_font: skip {}: {err:#}", path.display());
                last_err = Some(err);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        anyhow!("no usable system CJK font found (set POINTER_IM_CHART_FONT)")
    }))
}

fn load_font_file(path: &Path) -> Result<ImChartFont> {
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if !meta.is_file() {
        anyhow::bail!("not a file");
    }
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.is_empty() {
        anyhow::bail!("empty font file");
    }
    // fulgur always parses face index 0 — require CJK on that face.
    let face = ttf_parser::Face::parse(&bytes, 0).map_err(|e| anyhow!("parse face 0: {e}"))?;
    if face.glyph_index('中').is_none() && face.glyph_index('人').is_none() {
        anyhow::bail!("face 0 has no CJK glyphs");
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let identity = format!("{}|{}|{}", path.display(), meta.len(), mtime);
    Ok(ImChartFont {
        path: path.to_path_buf(),
        bytes,
        identity,
    })
}

fn system_font_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(target_os = "macos")]
    {
        out.extend([
            PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
            PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
            PathBuf::from("/System/Library/Fonts/MSYH.TTC"),
            PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
            PathBuf::from("/Library/Fonts/Arial Unicode.ttf"),
            PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc"),
            PathBuf::from("/System/Library/Fonts/Supplemental/Songti.ttc"),
        ]);
    }

    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let fonts = windir.join("Fonts");
        out.extend([
            fonts.join("msyh.ttc"),
            fonts.join("msyh.ttf"),
            fonts.join("msyhbd.ttc"),
            fonts.join("simhei.ttf"),
            fonts.join("simsun.ttc"),
            fonts.join("arialuni.ttf"),
        ]);
    }

    #[cfg(target_os = "linux")]
    {
        out.extend([
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJKsc-Regular.otf"),
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf"),
            PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
            PathBuf::from("/usr/share/fonts/truetype/arphic/uming.ttc"),
            PathBuf::from("/usr/share/fonts/TTF/NotoSansCJK-Regular.ttc"),
        ]);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_a_system_cjk_font_on_this_host() {
        match try_load_im_chart_font() {
            Ok(font) => {
                assert!(!font.bytes.is_empty());
                assert!(font.identity.contains('|'));
                let face = ttf_parser::Face::parse(&font.bytes, 0).unwrap();
                assert!(
                    face.glyph_index('中').is_some() || face.glyph_index('人').is_some(),
                    "loaded font missing CJK: {}",
                    font.path.display()
                );
            }
            Err(err) => {
                // Linux CI images may lack CJK fonts — do not fail the suite hard.
                eprintln!("im_chart_font unavailable (ok on minimal CI): {err:#}");
            }
        }
    }
}
