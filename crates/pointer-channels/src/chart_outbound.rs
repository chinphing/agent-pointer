//! Materialize Markdown `chartjs` / `chart` fences into PNG files for IM delivery.
//!
//! Desktop/Web keep interactive Chart.js rendering; IM platforms get raster
//! attachments via the existing `MEDIA:` pipeline (Hermes deliverable-style).
//!
//! Before rasterizing, JSON is normalized to match App chart styling:
//! soft series palette, white card background, and no function-looking strings.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use fulgur_chart::ir::Color;
use pointer_core::media::store::GENERATED_MEDIA_PREFIX;
use pointer_core::storage::app_data_dir;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const CHART_DIR: &str = "im-charts";
const DEFAULT_WIDTH: f64 = 800.0;
const DEFAULT_HEIGHT: f64 = 450.0;
const PNG_SCALE: f32 = 2.0;
/// Bump when palette / prep rules change so cached PNGs are not reused.
const STYLE_VERSION: &str = "pointer-im-chart-v2";

/// Same soft palette as `src/lib/markdownChart.ts` (`CHART_SERIES_PALETTE`).
const SERIES_PALETTE: &[(&str, &str, &str)] = &[
    // border, bar fill, line fill
    (
        "#4C8DDA",
        "rgba(76, 141, 218, 0.78)",
        "rgba(76, 141, 218, 0.14)",
    ),
    (
        "#E8A07A",
        "rgba(232, 160, 122, 0.82)",
        "rgba(232, 160, 122, 0.14)",
    ),
    (
        "#5DADE2",
        "rgba(93, 173, 226, 0.78)",
        "rgba(93, 173, 226, 0.14)",
    ),
    (
        "#C4A574",
        "rgba(196, 165, 116, 0.78)",
        "rgba(196, 165, 116, 0.14)",
    ),
    (
        "#7FBF9E",
        "rgba(127, 191, 158, 0.78)",
        "rgba(127, 191, 158, 0.14)",
    ),
    (
        "#A8B2C1",
        "rgba(168, 178, 193, 0.78)",
        "rgba(168, 178, 193, 0.14)",
    ),
    (
        "#C995A8",
        "rgba(201, 149, 168, 0.78)",
        "rgba(201, 149, 168, 0.14)",
    ),
    (
        "#A3B07A",
        "rgba(163, 176, 122, 0.78)",
        "rgba(163, 176, 122, 0.14)",
    ),
];

const SLICE_PALETTE: &[&str] = &[
    "#4C8DDA", "#E8A07A", "#5DADE2", "#C4A574", "#7FBF9E", "#A8B2C1", "#C995A8",
    "#A3B07A",
];

fn chart_fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?ms)^[ \t]*```(?:chartjs|chart)[ \t]*\r?\n(.*?)^[ \t]*```[ \t]*\r?$",
        )
        .expect("chart fence regex")
    })
}

/// Rewrite ` ```chartjs ` / ` ```chart ` fences into `MEDIA:<abs-path>` lines.
///
/// Failed renders keep the original fence and log a warning so IM still gets
/// the JSON fallback rather than dropping the content silently.
pub fn materialize_chartjs_fences_for_im(reply: &str) -> String {
    if reply.is_empty() || !reply.contains("```") {
        return reply.to_string();
    }
    let re = chart_fence_re();
    if !re.is_match(reply) {
        return reply.to_string();
    }

    let mut out = String::with_capacity(reply.len());
    let mut last = 0usize;
    let mut rendered = 0usize;
    let mut failed = 0usize;

    for caps in re.captures_iter(reply) {
        let full = caps.get(0).expect("full match");
        let body = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        out.push_str(&reply[last..full.start()]);
        match render_chart_fence_to_media_line(body) {
            Ok(media_line) => {
                rendered += 1;
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&media_line);
                out.push('\n');
            }
            Err(err) => {
                failed += 1;
                log::warn!("chart_outbound: render failed, keeping fence: {err:#}");
                out.push_str(full.as_str());
            }
        }
        last = full.end();
    }
    out.push_str(&reply[last..]);

    if rendered > 0 || failed > 0 {
        log::info!("chart_outbound: rendered={rendered} failed={failed}");
    }
    out
}

fn looks_like_js_callback(v: &Value) -> bool {
    v.as_str()
        .map(|s| s.contains("=>") || s.contains("function"))
        .unwrap_or(false)
}

/// Normalize Chart.js JSON so IM PNG styling matches App (palette + card bg).
///
/// Root `pointerPalette: false` keeps model series colors (user-requested).
/// Default / omitted applies the host soft palette.
pub(crate) fn prepare_chartjs_json_for_im(json_body: &str) -> Result<String> {
    let mut root: Value =
        serde_json::from_str(json_body.trim()).context("chartjs json parse")?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("chartjs root must be object"))?;

    let use_host_palette = obj
        .get("pointerPalette")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    // Host-only flag — not a Chart.js / fulgur field.
    obj.remove("pointerPalette");

    let chart_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("bar")
        .to_ascii_lowercase();
    let is_pie = chart_type == "pie" || chart_type == "doughnut";

    if let Some(data) = obj.get_mut("data").and_then(|d| d.as_object_mut()) {
        if let Some(datasets) = data.get_mut("datasets").and_then(|d| d.as_array_mut()) {
            for (index, ds) in datasets.iter_mut().enumerate() {
                let Some(ds_obj) = ds.as_object_mut() else {
                    continue;
                };
                // String scriptables crash / confuse rasterizers — drop them.
                ds_obj.remove("segment");
                for key in [
                    "borderColor",
                    "backgroundColor",
                    "pointBackgroundColor",
                    "pointBorderColor",
                ] {
                    if ds_obj.get(key).is_some_and(looks_like_js_callback) {
                        ds_obj.remove(key);
                    }
                }
                if ds_obj
                    .get("tension")
                    .and_then(|v| v.as_f64())
                    .is_some_and(|t| t > 0.0)
                {
                    ds_obj.insert("tension".into(), json!(0));
                }

                if !use_host_palette {
                    continue;
                }

                if is_pie {
                    let n = ds_obj
                        .get("data")
                        .and_then(|d| d.as_array())
                        .map(|a| a.len())
                        .unwrap_or(SLICE_PALETTE.len())
                        .max(1);
                    let slices: Vec<Value> = (0..n)
                        .map(|i| Value::String(SLICE_PALETTE[i % SLICE_PALETTE.len()].into()))
                        .collect();
                    ds_obj.insert("backgroundColor".into(), Value::Array(slices));
                    ds_obj.insert("borderColor".into(), Value::String("#ffffff".into()));
                    ds_obj.insert("borderWidth".into(), json!(2));
                } else {
                    let swatch = SERIES_PALETTE[index % SERIES_PALETTE.len()];
                    let ds_type = ds_obj
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&chart_type)
                        .to_ascii_lowercase();
                    let is_line = ds_type == "line";
                    let fill_color = if is_line { swatch.2 } else { swatch.1 };
                    ds_obj.insert("borderColor".into(), Value::String(swatch.0.into()));
                    ds_obj.insert(
                        "backgroundColor".into(),
                        Value::String(fill_color.into()),
                    );
                    ds_obj.insert(
                        "pointBackgroundColor".into(),
                        Value::String(swatch.0.into()),
                    );
                    ds_obj.insert("pointBorderColor".into(), Value::String(swatch.0.into()));
                    if !ds_obj.contains_key("borderWidth") {
                        ds_obj.insert("borderWidth".into(), json!(if is_line { 2 } else { 1 }));
                    }
                    // Object-form fill uses `above`/`below`, not backgroundColor.
                    if let Some(fill) = ds_obj.get_mut("fill").and_then(|f| f.as_object_mut()) {
                        if fill.get("above").and_then(|v| v.as_str()).is_some() {
                            fill.insert("above".into(), Value::String(fill_color.into()));
                        }
                        if fill.get("below").and_then(|v| v.as_str()).is_some() {
                            fill.insert("below".into(), Value::String(fill_color.into()));
                        }
                    }
                }
            }
        }
    }

    let options = obj
        .entry("options")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("options must be object"))?;

    // White card background (matches App export / chat card).
    options.insert("backgroundColor".into(), Value::String("#FFFFFF".into()));

    if let Some(plugins) = options.get_mut("plugins").and_then(|p| p.as_object_mut()) {
        plugins.remove("annotation");
    }

    if let Some(scales) = options.get_mut("scales").and_then(|s| s.as_object_mut()) {
        for (_key, scale) in scales.iter_mut() {
            let Some(scale_obj) = scale.as_object_mut() else {
                continue;
            };
            if let Some(ticks) = scale_obj.get_mut("ticks").and_then(|t| t.as_object_mut()) {
                if ticks.get("callback").is_some_and(looks_like_js_callback) {
                    ticks.remove("callback");
                }
            }
        }
    }

    serde_json::to_string(&root).context("serialize prepared chartjs")
}

fn render_chart_fence_to_media_line(json_body: &str) -> Result<String> {
    let json = json_body.trim();
    if json.is_empty() {
        anyhow::bail!("empty chart fence");
    }
    let prepared = prepare_chartjs_json_for_im(json)?;
    let mut spec = fulgur_chart::frontend::chartjs::parse(&prepared, false)
        .map_err(|e| anyhow::anyhow!("chartjs parse: {e}"))?;
    if spec.width <= 0.0 {
        spec.width = DEFAULT_WIDTH;
    }
    if spec.height <= 0.0 {
        spec.height = DEFAULT_HEIGHT;
    }
    // Reinforce white background even if the chartjs mapper ignored options.
    spec.theme.background = Some(Color {
        r: 255,
        g: 255,
        b: 255,
        a: 1.0,
    });

    let path = im_chart_png_path(&prepared)?;
    if path.is_file() {
        log::info!("chart_outbound: reuse {}", path.display());
        return Ok(format!("MEDIA:{}", path.display()));
    }

    let png = fulgur_chart::raster_direct::render_chart_to_png_default(&spec, PNG_SCALE)
        .map_err(|e| anyhow::anyhow!("chart png: {e}"))?;
    if png.is_empty() {
        anyhow::bail!("chart png empty");
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create chart dir {}", parent.display()))?;
    }
    fs::write(&path, &png).with_context(|| format!("write chart png {}", path.display()))?;
    log::info!("chart_outbound: wrote {}", path.display());
    Ok(format!("MEDIA:{}", path.display()))
}

fn im_chart_png_path(prepared_json: &str) -> Result<PathBuf> {
    let root = app_data_dir().context("app data dir")?;
    let mut hasher = Sha256::new();
    hasher.update(STYLE_VERSION.as_bytes());
    hasher.update(b"\n");
    hasher.update(prepared_json.as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Ok(root
        .join(GENERATED_MEDIA_PREFIX.trim_end_matches('/'))
        .join(CHART_DIR)
        .join(format!("{hex}.png")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pointer_core::media::outbound_reply::split_reply_media;

    #[test]
    fn leaves_plain_markdown_alone() {
        let src = "hello\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
        assert_eq!(materialize_chartjs_fences_for_im(src), src);
    }

    #[test]
    fn materializes_chartjs_fence_to_media_line() {
        let src = r#"Compare:

```chartjs
{"type":"bar","data":{"labels":["A","B"],"datasets":[{"data":[3,5]}]}}
```

Done."#;
        let out = materialize_chartjs_fences_for_im(src);
        assert!(
            !out.contains("```chartjs"),
            "fence should be replaced: {out}"
        );
        assert!(out.contains("MEDIA:"), "expected MEDIA line: {out}");
        assert!(out.contains("Compare:"));
        assert!(out.contains("Done."));

        let (visible, media) = split_reply_media(&out);
        assert!(visible.contains("Compare:"));
        assert!(!visible.contains("MEDIA:"));
        assert_eq!(media.len(), 1);
        let path = std::path::Path::new(&media[0]);
        assert!(path.exists(), "png missing: {}", path.display());
        let bytes = fs::read(path).expect("read png");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn keeps_invalid_fence() {
        let src = "```chartjs\n{not-json\n```";
        let out = materialize_chartjs_fences_for_im(src);
        assert!(out.contains("```chartjs"));
        assert!(!out.contains("MEDIA:"));
    }

    #[test]
    fn accepts_chart_lang_alias() {
        let src = "```chart\n{\"type\":\"pie\",\"data\":{\"labels\":[\"x\"],\"datasets\":[{\"data\":[1]}]}}\n```";
        let out = materialize_chartjs_fences_for_im(src);
        assert!(out.contains("MEDIA:"), "{out}");
        let (_, media) = split_reply_media(&out);
        assert_eq!(media.len(), 1);
        let _ = fs::remove_file(&media[0]);
    }

    #[test]
    fn prepare_applies_pointer_palette_and_strips_callbacks() {
        // Use r## so JSON color literals like "#e63946" do not terminate the raw string.
        let raw = r##"{
          "type":"line",
          "data":{"labels":["a","b"],"datasets":[{
            "data":[1,-2],
            "borderColor":"#e63946",
            "segment":{"borderDash":"ctx => [6,4]"},
            "tension":0.4
          }]},
          "options":{
            "plugins":{"annotation":{"annotations":{}}},
            "scales":{"y":{"ticks":{"callback":"v => v + 'x'"}}}
          }
        }"##;
        let prepared = prepare_chartjs_json_for_im(raw).expect("prepare");
        let v: Value = serde_json::from_str(&prepared).unwrap();
        let ds = &v["data"]["datasets"][0];
        assert_eq!(ds["borderColor"], "#4C8DDA");
        assert!(ds["backgroundColor"].as_str().unwrap().contains("76, 141, 218"));
        assert!(ds.get("segment").is_none());
        assert_eq!(ds["tension"], 0);
        assert_eq!(v["options"]["backgroundColor"], "#FFFFFF");
        assert!(v["options"]["plugins"].get("annotation").is_none());
        assert!(v["options"]["scales"]["y"]["ticks"].get("callback").is_none());
    }

    #[test]
    fn prepare_keeps_model_colors_when_pointer_palette_false() {
        let prepared = prepare_chartjs_json_for_im(
            r##"{
              "type":"line",
              "pointerPalette":false,
              "data":{"labels":["a","b"],"datasets":[{
                "data":[1,2],
                "borderColor":"#e63946",
                "backgroundColor":"rgba(230,57,70,0.12)",
                "fill":{"target":"origin","above":"rgba(230,57,70,0.15)"}
              }]}
            }"##,
        )
        .expect("prepare");
        let v: Value = serde_json::from_str(&prepared).unwrap();
        assert!(v.get("pointerPalette").is_none());
        let ds = &v["data"]["datasets"][0];
        assert_eq!(ds["borderColor"], "#e63946");
        assert_eq!(ds["backgroundColor"], "rgba(230,57,70,0.12)");
        assert_eq!(ds["fill"]["above"], "rgba(230,57,70,0.15)");
    }

    #[test]
    fn prepare_rewrites_object_fill_above_color() {
        let prepared = prepare_chartjs_json_for_im(
            r##"{
              "type":"line",
              "data":{"labels":["a","b"],"datasets":[{
                "data":[1,2],
                "fill":{"target":"origin","above":"rgba(230,57,70,0.15)"}
              }]}
            }"##,
        )
        .expect("prepare");
        let v: Value = serde_json::from_str(&prepared).unwrap();
        let above = v["data"]["datasets"][0]["fill"]["above"]
            .as_str()
            .unwrap_or("");
        assert!(
            above.contains("76, 141, 218"),
            "fill.above should use host palette, got {above}"
        );
    }

    #[test]
    fn neon_and_default_configs_share_styled_cache_key_inputs() {
        let a = prepare_chartjs_json_for_im(
            r##"{"type":"bar","data":{"labels":["A"],"datasets":[{"data":[1],"borderColor":"#ff0000"}]}}"##,
        )
        .unwrap();
        let b = prepare_chartjs_json_for_im(
            r##"{"type":"bar","data":{"labels":["A"],"datasets":[{"data":[1],"borderColor":"#00ff00"}]}}"##,
        )
        .unwrap();
        assert_eq!(a, b, "palette override should normalize colors");
    }
}
