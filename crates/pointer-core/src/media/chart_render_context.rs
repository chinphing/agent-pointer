//! API-only chart render notes (host palette) for LLM context.
//!
//! Not persisted in `msg.content` and not shown in the UI. Appended when
//! building OpenAI-compatible request messages from assistant history.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// Marker for host-applied chart styling injected into API payloads only.
pub const CHART_RENDER_MARKER: &str = "<!-- pointer-chart-render -->";

/// Same soft series borders as App (`CHART_SERIES_PALETTE`) / IM outbound.
const SERIES_BORDERS: &[&str] = &[
    "#4C8DDA", "#E8A07A", "#5DADE2", "#C4A574", "#7FBF9E", "#A8B2C1", "#C995A8", "#A3B07A",
];

fn chart_fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?ms)^[ \t]*```(?:chartjs|chart)[ \t]*\r?\n(.*?)^[ \t]*```[ \t]*\r?$")
            .expect("chart fence regex")
    })
}

/// Build a short host-render note for every chart fence in `content`.
pub fn format_chart_render_api_manifest(content: &str) -> String {
    if content.is_empty() || !content.contains("```") {
        return String::new();
    }
    let re = chart_fence_re();
    let mut lines: Vec<String> = Vec::new();
    for (idx, caps) in re.captures_iter(content).enumerate() {
        let body = caps.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        match summarize_chart_fence(idx + 1, body) {
            Ok(line) => lines.push(line),
            Err(err) => {
                log::debug!("chart_render_context: skip fence {}: {err}", idx + 1);
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = vec![
        CHART_RENDER_MARKER.to_string(),
        "Host-applied chart styling (API-only; not shown in the UI). When referring to series colors in this turn, use these values — not neon colors left in the fence JSON.".to_string(),
    ];
    out.extend(lines);
    out.join("\n")
}

/// Append chart-render notes to assistant text for the OpenAI API payload.
pub fn append_chart_render_api_context(content: &str) -> String {
    let manifest = format_chart_render_api_manifest(content);
    if manifest.is_empty() {
        return content.to_string();
    }
    if content.trim().is_empty() {
        manifest
    } else {
        format!("{}\n\n{manifest}", content.trim_end())
    }
}

fn summarize_chart_fence(index: usize, json_body: &str) -> Result<String, String> {
    let root: Value = serde_json::from_str(json_body).map_err(|e| format!("json parse: {e}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| "root not object".to_string())?;
    let chart_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("bar")
        .to_ascii_lowercase();
    let use_host = obj
        .get("pointerPalette")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let datasets = obj
        .get("data")
        .and_then(|d| d.get("datasets"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    if datasets.is_empty() {
        return Err("no datasets".into());
    }

    let is_pie = chart_type == "pie" || chart_type == "doughnut";
    if !use_host {
        let mut colors: Vec<String> = Vec::new();
        for ds in &datasets {
            if let Some(c) = color_preview(ds.get("borderColor"))
                .or_else(|| color_preview(ds.get("backgroundColor")))
            {
                colors.push(c);
            }
        }
        if colors.is_empty() {
            return Ok(format!(
                "- chart {index} ({chart_type}): custom colors (pointerPalette=false)"
            ));
        }
        return Ok(format!(
            "- chart {index} ({chart_type}): custom colors (pointerPalette=false): {}",
            colors.join(", ")
        ));
    }

    if is_pie {
        let n = datasets
            .first()
            .and_then(|ds| ds.get("data"))
            .and_then(|d| d.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
            .max(1);
        return Ok(format!(
            "- chart {index} ({chart_type}): host soft slice palette ({n} slices)"
        ));
    }

    let mut parts: Vec<String> = Vec::new();
    for (i, ds) in datasets.iter().enumerate() {
        let border = SERIES_BORDERS[i % SERIES_BORDERS.len()];
        let label = ds
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(label) = label {
            parts.push(format!("{border} ({label})"));
        } else {
            parts.push(border.to_string());
        }
    }
    Ok(format!(
        "- chart {index} ({chart_type}): host series colors: {}",
        parts.join(", ")
    ))
}

fn color_preview(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.as_str() {
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        // Keep short: full rgba strings are fine but truncate extreme length.
        if t.len() > 48 {
            return Some(format!("{}…", &t[..45]));
        }
        return Some(t.to_string());
    }
    if let Some(arr) = v.as_array() {
        let mut out: Vec<String> = Vec::new();
        for item in arr.iter().take(4) {
            if let Some(s) = item.as_str() {
                out.push(s.trim().to_string());
            }
        }
        if out.is_empty() {
            return None;
        }
        if arr.len() > out.len() {
            out.push("…".into());
        }
        return Some(out.join("/"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_host_palette_note_for_chart_fence() {
        let src = r#"See trend:

```chartjs
{"type":"line","data":{"labels":["a"],"datasets":[{"label":"人口","data":[1]}]}}
```
"#;
        let out = append_chart_render_api_context(src);
        assert!(out.contains(CHART_RENDER_MARKER));
        assert!(out.contains("#4C8DDA"));
        assert!(out.contains("人口"));
        assert!(out.contains("See trend:"));
        // Original fence unchanged in the visible prefix.
        assert!(out.contains("```chartjs"));
    }

    #[test]
    fn notes_custom_colors_when_pointer_palette_false() {
        // r## so JSON "#hex" does not terminate the raw string.
        let src = r##"
```chartjs
{"type":"bar","pointerPalette":false,"data":{"labels":["A"],"datasets":[{"data":[1],"borderColor":"#e63946"}]}}
```
"##;
        let out = append_chart_render_api_context(src);
        assert!(out.contains("pointerPalette=false"));
        assert!(out.contains("#e63946"));
        assert!(!out.contains("#4C8DDA"));
    }

    #[test]
    fn leaves_plain_markdown_alone() {
        let src = "hello\n\n| a | b |\n";
        assert_eq!(append_chart_render_api_context(src), src);
    }
}
