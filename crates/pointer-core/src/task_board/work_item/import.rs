//! Import work_items from CSV / JSONL / XLSX files under workspace.

use super::model::{draft_from_value, WorkItemDraft};
use crate::media::resolve_media_ref;
use anyhow::{anyhow, Context, Result};
use calamine::{open_workbook_auto, Reader};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const MAX_IMPORT_ROWS: usize = 50_000;

fn normalize_header_label(h: &str) -> String {
    h.trim().to_ascii_lowercase()
}

fn is_index_column_header(h: &str) -> bool {
    let t = h.trim();
    let lower = normalize_header_label(t);
    matches!(
        lower.as_str(),
        "id" | "idx" | "index" | "seq" | "sequence" | "no" | "num" | "number" | "#"
    ) || matches!(t, "序号" | "编号")
}

/// Map spreadsheet headers to the display `title` column (UI shows `WorkItem.title`).
fn title_column_index(headers: &[String]) -> Option<usize> {
    const EXACT: &[&str] = &[
        "title", "name", "label", "item", "task", "名称", "标题",
    ];
    for (i, h) in headers.iter().enumerate() {
        let t = h.trim();
        let lower = normalize_header_label(t);
        if EXACT.iter().any(|a| lower == *a || t == *a) {
            return Some(i);
        }
    }
    for (i, h) in headers.iter().enumerate() {
        if is_index_column_header(h) {
            continue;
        }
        let t = h.trim();
        let lower = normalize_header_label(t);
        if t.contains("名称")
            || lower.contains("name")
            || lower.contains("title")
            || lower.contains("label")
        {
            return Some(i);
        }
    }
    headers
        .iter()
        .position(|h| !is_index_column_header(h.trim()))
}

fn key_column_index(headers: &[String]) -> Option<usize> {
    headers.iter().position(|h| {
        let t = h.trim();
        let lower = normalize_header_label(t);
        if matches!(lower.as_str(), "target_key" | "targetkey" | "key" | "target") {
            return true;
        }
        lower == "id" && !is_index_column_header(t)
    })
}

fn cell_as_title(
    headers: &[String],
    cells: &[String],
    title_idx: Option<usize>,
    key_idx: Option<usize>,
) -> String {
    if let Some(i) = title_idx {
        if let Some(s) = cells.get(i) {
            let t = s.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    for (i, cell) in cells.iter().enumerate() {
        if Some(i) == key_idx {
            continue;
        }
        if headers.get(i).is_some_and(|h| is_index_column_header(h)) {
            continue;
        }
        let t = cell.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    cells
        .first()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Resolve a work_items import path — same rules as general-agent media refs
/// (`pointer-media://`, storage rel, absolute/`~/`, then workspace-relative fallback).
pub fn resolve_import_path(workspace_root: &str, path: &str) -> Result<PathBuf> {
    let raw = path.trim();
    if raw.is_empty() {
        return Err(anyhow!("work_items_source: path is empty"));
    }

    if let Ok(resolved) = resolve_media_ref(raw) {
        if resolved.is_file() {
            return Ok(resolved);
        }
        return Err(anyhow!(
            "work_items_source: not a file: {}",
            resolved.display()
        ));
    }

    let p = Path::new(raw);
    let resolved = if p.is_absolute() {
        p.to_path_buf()
    } else if !workspace_root.trim().is_empty() {
        Path::new(workspace_root.trim()).join(p)
    } else {
        p.to_path_buf()
    };
    if !resolved.is_file() {
        return Err(anyhow!(
            "work_items_source: file not found: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

pub fn detect_format(path: &Path, explicit: Option<&str>) -> Result<&'static str> {
    if let Some(fmt) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return match fmt.to_ascii_lowercase().as_str() {
            "csv" => Ok("csv"),
            "jsonl" | "ndjson" => Ok("jsonl"),
            "xlsx" | "xls" => Ok("xlsx"),
            other => Err(anyhow!("work_items_source: unsupported format {other}")),
        };
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => Ok("csv"),
        "jsonl" | "ndjson" => Ok("jsonl"),
        "xlsx" | "xls" => Ok("xlsx"),
        ext => Err(anyhow!("work_items_source: cannot detect format from .{ext}")),
    }
}

pub fn import_drafts_from_file(path: &Path, format: &str) -> Result<Vec<WorkItemDraft>> {
    let drafts = match format {
        "jsonl" => import_jsonl(path)?,
        "csv" => import_csv(path)?,
        "xlsx" => import_xlsx(path)?,
        other => return Err(anyhow!("work_items_source: unsupported format {other}")),
    };
    if drafts.is_empty() {
        return Err(anyhow!("work_items_source: no rows imported from {}", path.display()));
    }
    if drafts.len() > MAX_IMPORT_ROWS {
        return Err(anyhow!(
            "work_items_source: exceeds MAX_IMPORT_ROWS ({MAX_IMPORT_ROWS})"
        ));
    }
    log::info!(
        "work_items: imported path={} format={} rows={}",
        path.display(),
        format,
        drafts.len()
    );
    Ok(drafts)
}

fn import_jsonl(path: &Path) -> Result<Vec<WorkItemDraft>> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("read line {}", i + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(trimmed)
            .with_context(|| format!("jsonl line {} invalid json", i + 1))?;
        if let Some(d) = draft_from_value(&v) {
            out.push(d);
        }
    }
    Ok(out)
}

fn import_csv(path: &Path) -> Result<Vec<WorkItemDraft>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("csv open {}", path.display()))?;
    let headers: Vec<String> = rdr
        .headers()
        .map(|h| h.iter().map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();
    let title_idx = title_column_index(&headers);
    let key_idx = key_column_index(&headers);
    let mut out = Vec::new();
    for row in rdr.records() {
        let row = row.with_context(|| "csv row")?;
        let cells: Vec<String> = row.iter().map(|c| c.trim().to_string()).collect();
        let title = cell_as_title(&headers, &cells, title_idx, key_idx);
        if title.is_empty() {
            continue;
        }
        let target_key = key_idx
            .and_then(|i| row.get(i))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let mut payload = json!({});
        for (i, cell) in row.iter().enumerate() {
            if Some(i) == title_idx || Some(i) == key_idx {
                continue;
            }
            let key = headers.get(i).cloned().unwrap_or_else(|| format!("col_{i}"));
            if !cell.trim().is_empty() {
                payload[key] = json!(cell.trim());
            }
        }
        out.push(WorkItemDraft {
            title,
            target_key,
            payload,
        });
    }
    Ok(out)
}

fn import_xlsx(path: &Path) -> Result<Vec<WorkItemDraft>> {
    let mut workbook = open_workbook_auto(path)
        .with_context(|| format!("xlsx open {}", path.display()))?;
    let sheet_names = workbook.sheet_names().to_vec();
    let first = sheet_names
        .first()
        .ok_or_else(|| anyhow!("xlsx: no sheets"))?;
    let range = workbook
        .worksheet_range(first)
        .map_err(|e| anyhow!("xlsx read sheet: {e}"))?;
    let mut rows = range.rows();
    let header_row = rows
        .next()
        .ok_or_else(|| anyhow!("xlsx: empty sheet"))?;
    let headers: Vec<String> = header_row
        .iter()
        .map(|c| format!("{c}").trim().to_string())
        .collect();
    let title_idx = title_column_index(&headers);
    let key_idx = key_column_index(&headers);
    let mut out = Vec::new();
    for row in rows {
        let cells: Vec<String> = row.iter().map(|c| format!("{c}").trim().to_string()).collect();
        let title = cell_as_title(&headers, &cells, title_idx, key_idx);
        if title.is_empty() {
            continue;
        }
        let target_key = key_idx
            .and_then(|i| cells.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let mut payload = json!({});
        for (i, cell) in cells.iter().enumerate() {
            if Some(i) == title_idx || Some(i) == key_idx {
                continue;
            }
            let key = headers.get(i).cloned().unwrap_or_else(|| format!("col_{i}"));
            if !cell.is_empty() {
                payload[key] = json!(cell);
            }
        }
        out.push(WorkItemDraft {
            title,
            target_key,
            payload,
        });
    }
    Ok(out)
}

pub fn work_items_source_path_from_value(
    source: &Value,
    workspace_root: &str,
) -> Result<std::path::PathBuf> {
    let path = if let Some(s) = source.as_str() {
        s
    } else {
        source
            .get("path")
            .or_else(|| source.get("file"))
            .or_else(|| source.get("ref"))
            .and_then(|x| x.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "work_items_source: pass a string path or media ref (e.g. \"/path/list.xlsx\" or \"pointer-media://…\"), not an object without path/ref"
                )
            })?
    };
    resolve_import_path(workspace_root, path)
}

pub fn drafts_from_source_value(v: &Value, workspace_root: &str) -> Result<Vec<WorkItemDraft>> {
    let resolved = work_items_source_path_from_value(v, workspace_root)?;
    let format = v.get("format").and_then(|x| x.as_str());
    let fmt = detect_format(&resolved, format)?;
    import_drafts_from_file(&resolved, fmt)
}

/// Re-import from an already-resolved absolute path (reload / rehydrate).
pub fn drafts_from_resolved_path(path: &Path) -> Result<Vec<WorkItemDraft>> {
    let fmt = detect_format(path, None)?;
    import_drafts_from_file(path, fmt)
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn work_items_source_accepts_plain_string_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.csv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "title").unwrap();
        writeln!(f, "Beijing").unwrap();
        let resolved =
            work_items_source_path_from_value(&json!(path.display().to_string()), "").unwrap();
        assert_eq!(resolved, path);
    }

    #[test]
    fn csv_import_reads_title_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.csv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "title,target_key").unwrap();
        writeln!(f, "App One,app1").unwrap();
        writeln!(f, "App Two,app2").unwrap();
        let drafts = import_drafts_from_file(&path, "csv").unwrap();
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].title, "App One");
        assert_eq!(drafts[0].target_key.as_deref(), Some("app1"));
    }

    #[test]
    fn title_column_index_matches_name_like_headers() {
        assert_eq!(title_column_index(&["序号".into(), "城市名称".into()]), Some(1));
        assert_eq!(title_column_index(&["seq".into(), "product_name".into()]), Some(1));
        assert_eq!(title_column_index(&["#".into(), "title".into()]), Some(1));
        assert_eq!(title_column_index(&["序号".into(), "备注".into()]), Some(1));
    }

    #[test]
    fn csv_import_uses_city_name_not_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cities.csv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "序号,城市名称").unwrap();
        writeln!(f, "1,北京").unwrap();
        writeln!(f, "2,上海").unwrap();
        let drafts = import_drafts_from_file(&path, "csv").unwrap();
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].title, "北京");
        assert_eq!(drafts[1].title, "上海");
    }
}
