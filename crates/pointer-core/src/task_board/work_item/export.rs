//! Export work_items to xlsx / csv / txt / jsonl under workspace.

use super::model::{WorkItem, WorkItemStatus};
use super::store::WorkItemStore;
use crate::task_board::model::{BoardDocument, ItemStatus};
use anyhow::{anyhow, Context, Result};
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const DEFAULT_COLUMNS: &[&str] = &["seq", "id", "title", "status", "result_summary"];

#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub campaign_id: String,
    pub batch_ids: Vec<String>,
    pub format: String,
    pub output_path: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportOutcome {
    pub ok: bool,
    pub path: String,
    pub format: String,
    pub row_count: u32,
    pub bytes: u64,
}

pub fn export_work_items(
    store: &WorkItemStore,
    doc: &BoardDocument,
    workspace_root: &str,
    req: ExportRequest,
) -> Result<ExportOutcome> {
    let batch_ids = if req.batch_ids.is_empty() {
        doc.board
            .iter()
            .filter(|r| r.has_work_items())
            .map(|r| r.id.clone())
            .collect()
    } else {
        req.batch_ids.clone()
    };
    if batch_ids.is_empty() {
        return Err(anyhow!("work_items_export: no work-item batches on board"));
    }
    validate_delivery_prerequisites(doc, &batch_ids)?;

    let mut items: Vec<WorkItem> = Vec::new();
    for batch_id in &batch_ids {
        items.extend(store.items_in_batch(&req.campaign_id, batch_id));
    }
    items.sort_by_key(|i| i.seq);
    if items.is_empty() {
        return Err(anyhow!("work_items_export: no work_items rows in store"));
    }

    let format = normalize_format(&req.format);
    let path = resolve_output_path(workspace_root, &req.campaign_id, format, req.output_path.as_deref())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let columns = if req.columns.is_empty() {
        DEFAULT_COLUMNS.iter().map(|s| s.to_string()).collect()
    } else {
        req.columns
    };
    let bytes = match format {
        "xlsx" => write_xlsx(&path, &items, &columns)?,
        "csv" => write_csv(&path, &items, &columns)?,
        "jsonl" => write_jsonl(&path, &items)?,
        "txt" => write_txt(&path, &items)?,
        other => return Err(anyhow!("work_items_export: unsupported format {other}")),
    };
    log::info!(
        "work_items: export campaign_id={} path={} format={} rows={} bytes={bytes}",
        req.campaign_id,
        path.display(),
        format,
        items.len()
    );
    Ok(ExportOutcome {
        ok: true,
        path: path.to_string_lossy().into_owned(),
        format: format.to_string(),
        row_count: items.len() as u32,
        bytes,
    })
}

fn normalize_format(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "csv" => "csv",
        "txt" | "text" => "txt",
        "jsonl" | "ndjson" => "jsonl",
        _ => "xlsx",
    }
}

fn resolve_output_path(
    workspace_root: &str,
    campaign_id: &str,
    format: &str,
    override_path: Option<&str>,
) -> Result<PathBuf> {
    if let Some(p) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        let path = Path::new(p);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new(workspace_root.trim()).join(path)
        };
        let root = Path::new(workspace_root.trim()).canonicalize().unwrap_or_else(|_| PathBuf::from(workspace_root));
        let resolved_canon = resolved.canonicalize().unwrap_or(resolved.clone());
        if !resolved_canon.starts_with(&root) {
            return Err(anyhow!("work_items_export: output_path must stay under workspace"));
        }
        return Ok(resolved);
    }
    Ok(Path::new(workspace_root.trim())
        .join("exports")
        .join(format!("{campaign_id}_results.{format}")))
}

fn validate_delivery_prerequisites(doc: &BoardDocument, batch_ids: &[String]) -> Result<()> {
    for batch_id in batch_ids {
        let Some(row) = doc.board.iter().find(|r| r.id == *batch_id) else {
            return Err(anyhow!("work_items_export: batch {batch_id} not on board"));
        };
        if !matches!(row.status, ItemStatus::Done | ItemStatus::Failed | ItemStatus::Cancelled) {
            return Err(anyhow!(
                "work_items_export: batch {batch_id} not terminal (status={:?})",
                row.status
            ));
        }
    }
    Ok(())
}

fn result_summary(item: &WorkItem) -> String {
    item.result_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.get("summary").and_then(|x| x.as_str()).map(str::to_string))
        .unwrap_or_default()
}

fn cell_value(item: &WorkItem, col: &str) -> String {
    match col {
        "seq" => item.seq.to_string(),
        "id" => item.id.clone(),
        "title" => item.title.clone(),
        "status" => item.status.as_str().to_string(),
        "result_summary" => result_summary(item),
        "batch_id" => item.batch_id.clone(),
        other => {
            if let Ok(v) = serde_json::from_str::<Value>(&item.payload_json) {
                v.get(other)
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        _ => x.to_string(),
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            }
        }
    }
}

fn write_xlsx(path: &Path, items: &[WorkItem], columns: &[String]) -> Result<u64> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    write_sheet(worksheet, items, columns)?;
    workbook.save(path).with_context(|| format!("xlsx save {}", path.display()))?;
    Ok(fs::metadata(path)?.len())
}

fn write_sheet(worksheet: &mut Worksheet, items: &[WorkItem], columns: &[String]) -> Result<()> {
    let header_fmt = Format::new().set_bold();
    for (c, col) in columns.iter().enumerate() {
        worksheet.write_string_with_format(0, c as u16, col, &header_fmt)?;
    }
    for (r, item) in items.iter().enumerate() {
        for (c, col) in columns.iter().enumerate() {
            worksheet.write_string((r + 1) as u32, c as u16, &cell_value(item, col))?;
        }
    }
    Ok(())
}

fn write_csv(path: &Path, items: &[WorkItem], columns: &[String]) -> Result<u64> {
    let mut wtr = csv::WriterBuilder::new().from_path(path)?;
    wtr.write_record(columns.iter().map(String::as_str))?;
    for item in items {
        let row: Vec<String> = columns.iter().map(|c| cell_value(item, c)).collect();
        wtr.write_record(row.iter().map(String::as_str))?;
    }
    wtr.flush()?;
    Ok(fs::metadata(path)?.len())
}

fn write_jsonl(path: &Path, items: &[WorkItem]) -> Result<u64> {
    let mut file = File::create(path)?;
    for item in items {
        let line = serde_json::json!({
            "seq": item.seq,
            "id": item.id,
            "batch_id": item.batch_id,
            "title": item.title,
            "status": item.status.as_str(),
            "result_summary": result_summary(item),
            "payload": serde_json::from_str::<Value>(&item.payload_json).unwrap_or(Value::Null),
        });
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
    }
    Ok(fs::metadata(path)?.len())
}

fn write_txt(path: &Path, items: &[WorkItem]) -> Result<u64> {
    let mut file = File::create(path)?;
    for item in items {
        writeln!(
            file,
            "{}\t{}\t{}\t{}",
            item.seq,
            item.id,
            item.title,
            item.status.as_str()
        )?;
    }
    Ok(fs::metadata(path)?.len())
}
