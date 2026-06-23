//! Import work_items from CSV / JSONL / XLSX files under workspace.

use super::model::{draft_from_value, WorkItemDraft};
use anyhow::{anyhow, Context, Result};
use calamine::{open_workbook_auto, Reader};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const MAX_IMPORT_ROWS: usize = 50_000;

pub fn resolve_import_path(workspace_root: &str, path: &str) -> Result<PathBuf> {
    let raw = path.trim();
    if raw.is_empty() {
        return Err(anyhow!("work_items_source: path is empty"));
    }
    let p = Path::new(raw);
    let resolved = if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(workspace_root.trim()).join(p)
    };
    if !resolved.is_file() {
        return Err(anyhow!("work_items_source: file not found: {}", resolved.display()));
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
        .map(|h| h.iter().map(|s| s.trim().to_ascii_lowercase()).collect())
        .unwrap_or_default();
    let title_idx = headers.iter().position(|h| h == "title");
    let key_idx = headers
        .iter()
        .position(|h| h == "target_key" || h == "targetkey" || h == "id");
    let mut out = Vec::new();
    for row in rdr.records() {
        let row = row.with_context(|| "csv row")?;
        let title = title_idx
            .and_then(|i| row.get(i))
            .or_else(|| row.get(0))
            .unwrap_or("")
            .trim()
            .to_string();
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
        .map(|c| format!("{c}").trim().to_ascii_lowercase())
        .collect();
    let title_idx = headers.iter().position(|h| h == "title");
    let key_idx = headers
        .iter()
        .position(|h| h == "target_key" || h == "targetkey" || h == "id");
    let mut out = Vec::new();
    for row in rows {
        let cells: Vec<String> = row.iter().map(|c| format!("{c}").trim().to_string()).collect();
        let title = title_idx
            .and_then(|i| cells.get(i))
            .or_else(|| cells.first())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
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

pub fn drafts_from_source_value(v: &Value, workspace_root: &str) -> Result<Vec<WorkItemDraft>> {
    let path = v
        .get("path")
        .or_else(|| v.get("file"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("work_items_source: missing path"))?;
    let format = v.get("format").and_then(|x| x.as_str());
    let resolved = resolve_import_path(workspace_root, path)?;
    let fmt = detect_format(&resolved, format)?;
    import_drafts_from_file(&resolved, fmt)
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use std::io::Write;

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
}
