//! Extract plain text from Office Open XML documents (docx, xlsx).

use anyhow::{Context, Result};
use std::io::{Cursor, Read};
use zip::ZipArchive;

const MAX_OFFICE_TEXT_BYTES: usize = 256 * 1024;

pub fn is_docx(mime: &str, file_name: &str, bytes: &[u8]) -> bool {
    let mime = mime.trim().to_ascii_lowercase();
    if mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document" {
        return true;
    }
    if file_name.to_ascii_lowercase().ends_with(".docx") {
        return true;
    }
    zip_contains(bytes, "word/document.xml")
}

pub fn is_xlsx(mime: &str, file_name: &str, bytes: &[u8]) -> bool {
    let mime = mime.trim().to_ascii_lowercase();
    if mime == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" {
        return true;
    }
    if file_name.to_ascii_lowercase().ends_with(".xlsx") {
        return true;
    }
    zip_contains(bytes, "xl/workbook.xml")
}

fn zip_contains(bytes: &[u8], entry: &str) -> bool {
    let Ok(mut zip) = ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    let found = zip.by_name(entry).is_ok();
    found
}

pub fn extract_docx_text(bytes: &[u8], file_name: &str) -> Result<String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .with_context(|| format!("open docx zip {file_name}"))?;
    let mut doc = zip
        .by_name("word/document.xml")
        .with_context(|| format!("docx missing word/document.xml: {file_name}"))?;
    let mut xml = String::new();
    doc.read_to_string(&mut xml)
        .with_context(|| format!("read word/document.xml for {file_name}"))?;
    let text = extract_w_t_text(&xml);
    if text.trim().is_empty() {
        anyhow::bail!("docx contains no extractable text: {file_name}");
    }
    truncate_office_text(text, "docx", file_name)
}

pub fn extract_xlsx_text(bytes: &[u8], file_name: &str) -> Result<String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .with_context(|| format!("open xlsx zip {file_name}"))?;
    let shared = read_shared_strings(&mut zip, file_name)?;
    let sheet_paths = list_worksheet_paths(&mut zip);
    if sheet_paths.is_empty() {
        anyhow::bail!("xlsx missing worksheets: {file_name}");
    }

    let mut out = String::new();
    for (idx, path) in sheet_paths.iter().enumerate() {
        let mut sheet = zip
            .by_name(path)
            .with_context(|| format!("read worksheet {path} in {file_name}"))?;
        let mut xml = String::new();
        sheet
            .read_to_string(&mut xml)
            .with_context(|| format!("read worksheet {path} for {file_name}"))?;
        let label = worksheet_label(path, idx);
        let rows = extract_sheet_rows(&xml, &shared);
        let non_empty: Vec<_> = rows
            .into_iter()
            .filter(|row| row.iter().any(|cell| !cell.trim().is_empty()))
            .collect();
        if non_empty.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("=== {label} ==="));
        for row in non_empty {
            out.push('\n');
            out.push_str(&row.join("\t"));
        }
    }

    if out.trim().is_empty() {
        anyhow::bail!("xlsx contains no extractable text: {file_name}");
    }
    truncate_office_text(out, "xlsx", file_name)
}

fn truncate_office_text(text: String, kind: &str, file_name: &str) -> Result<String> {
    if text.len() > MAX_OFFICE_TEXT_BYTES {
        log::warn!(
            "{kind} {file_name} text exceeds {} bytes; truncating",
            MAX_OFFICE_TEXT_BYTES
        );
        Ok(crate::text_util::truncate_bytes(
            &text,
            MAX_OFFICE_TEXT_BYTES,
        ))
    } else {
        Ok(text)
    }
}

fn read_shared_strings(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    file_name: &str,
) -> Result<Vec<String>> {
    let Ok(mut file) = zip.by_name("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    let mut xml = String::new();
    file.read_to_string(&mut xml)
        .with_context(|| format!("read xl/sharedStrings.xml for {file_name}"))?;
    Ok(extract_shared_strings(&xml))
}

fn list_worksheet_paths(zip: &mut ZipArchive<Cursor<&[u8]>>) -> Vec<String> {
    let mut paths: Vec<String> = (0..zip.len())
        .filter_map(|i| {
            let file = zip.by_index(i).ok()?;
            let name = file.name().to_string();
            if name.starts_with("xl/worksheets/") && name.ends_with(".xml") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    paths.sort();
    paths
}

fn worksheet_label(path: &str, idx: usize) -> String {
    path.rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".xml"))
        .map(str::to_string)
        .unwrap_or_else(|| format!("Sheet{}", idx + 1))
}

fn extract_shared_strings(xml: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut rest = xml;
    while let Some(si_start) = rest.find("<si") {
        let after_tag = match rest[si_start..].find('>') {
            Some(off) => si_start + off + 1,
            None => break,
        };
        let tail = &rest[after_tag..];
        let si_end = match tail.find("</si>") {
            Some(off) => off,
            None => break,
        };
        strings.push(extract_concat_t_tags(&tail[..si_end]));
        rest = &tail[si_end + 5..];
    }
    strings
}

fn extract_sheet_rows(xml: &str, shared: &[String]) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut rest = xml;
    while let Some(row_start) = rest.find("<row") {
        let after_tag = match rest[row_start..].find('>') {
            Some(off) => row_start + off + 1,
            None => break,
        };
        let tail = &rest[after_tag..];
        let row_end = match tail.find("</row>") {
            Some(off) => off,
            None => break,
        };
        rows.push(extract_row_cells(&tail[..row_end], shared));
        rest = &tail[row_end + 6..];
    }
    rows
}

fn extract_row_cells(row_xml: &str, shared: &[String]) -> Vec<String> {
    let mut cells = Vec::new();
    let mut rest = row_xml;
    while let Some(c_start) = rest.find("<c") {
        if !is_cell_tag_start(&rest[c_start..]) {
            rest = &rest[c_start + 2..];
            continue;
        }
        let after_tag = match rest[c_start..].find('>') {
            Some(off) => c_start + off + 1,
            None => break,
        };
        let tail = &rest[after_tag..];
        if rest[c_start..after_tag].ends_with("/>") {
            cells.push(String::new());
            rest = tail;
            continue;
        }
        let cell_end = match tail.find("</c>") {
            Some(off) => off,
            None => break,
        };
        let cell_xml = &rest[c_start..after_tag + cell_end + 4];
        cells.push(extract_cell_text(cell_xml, shared));
        rest = &tail[cell_end + 4..];
    }
    cells
}

fn is_cell_tag_start(fragment: &str) -> bool {
    if !fragment.starts_with("<c") {
        return false;
    }
    let rest = &fragment[2..];
    rest.starts_with(' ')
        || rest.starts_with('>')
        || rest.starts_with("r=")
        || rest.starts_with('\t')
        || rest.starts_with('\n')
        || rest.starts_with('\r')
}

fn extract_cell_text(cell_xml: &str, shared: &[String]) -> String {
    match extract_xml_attr(cell_xml, "t").as_deref() {
        Some("s") => extract_v_value(cell_xml)
            .and_then(|v| v.parse::<usize>().ok())
            .and_then(|idx| shared.get(idx).cloned())
            .unwrap_or_default(),
        Some("inlineStr") => extract_concat_t_tags(cell_xml),
        Some("b") => extract_v_value(cell_xml)
            .map(|v| {
                if v == "1" {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                }
            })
            .unwrap_or_default(),
        Some("str") => extract_v_value(cell_xml).unwrap_or_default(),
        _ => extract_v_value(cell_xml).unwrap_or_default(),
    }
}

fn extract_xml_attr(fragment: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = fragment.find(&needle)? + needle.len();
    let tail = &fragment[start..];
    let end = tail.find('"')?;
    Some(unescape_xml(&tail[..end]))
}

fn extract_v_value(fragment: &str) -> Option<String> {
    let start = fragment.find("<v>")? + 3;
    let tail = &fragment[start..];
    let end = tail.find("</v>")?;
    Some(unescape_xml(&tail[..end]))
}

fn extract_w_t_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<w:t") {
        let after_tag = match rest[start..].find('>') {
            Some(off) => start + off + 1,
            None => break,
        };
        let tail = &rest[after_tag..];
        let end = match tail.find("</w:t>") {
            Some(off) => off,
            None => break,
        };
        let chunk = &tail[..end];
        if !chunk.is_empty() {
            if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(chunk);
        }
        rest = &tail[end + 6..];
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_concat_t_tags(fragment: &str) -> String {
    let mut out = String::new();
    let mut rest = fragment;
    while let Some(start) = rest.find("<t") {
        if !is_t_tag_start(&rest[start..]) {
            rest = &rest[start + 2..];
            continue;
        }
        let after_tag = match rest[start..].find('>') {
            Some(off) => start + off + 1,
            None => break,
        };
        let tail = &rest[after_tag..];
        let end = match tail.find("</t>") {
            Some(off) => off,
            None => break,
        };
        out.push_str(&unescape_xml(&tail[..end]));
        rest = &tail[end + 4..];
    }
    out
}

fn is_t_tag_start(fragment: &str) -> bool {
    if !fragment.starts_with("<t") {
        return false;
    }
    let rest = &fragment[2..];
    rest.starts_with('>')
        || rest.starts_with(' ')
        || rest.starts_with(" xml:space")
        || rest.starts_with('\t')
        || rest.starts_with('\n')
        || rest.starts_with('\r')
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};
    use zip::CompressionMethod;

    #[test]
    fn extracts_w_t_runs() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t> world</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(extract_w_t_text(xml), "Hello world");
    }

    #[test]
    fn extracts_shared_strings_and_sheet_rows() {
        let bytes = minimal_xlsx_bytes(
            br#"<?xml version="1.0"?><sst><si><t>Name</t></si><si><t>Alice</t></si><si><t>30</t></si></sst>"#,
            br#"<?xml version="1.0"?><worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c><c r="B2"><v>30</v></c></row></sheetData></worksheet>"#,
        );
        let text = extract_xlsx_text(&bytes, "sample.xlsx").unwrap();
        assert!(text.contains("=== sheet1 ==="));
        assert!(text.contains("Name\tAlice"));
        assert!(text.contains("2\t30"));
    }

    #[test]
    fn extracts_inline_str_cells() {
        let bytes = minimal_xlsx_bytes(
            br#"<sst/>"#,
            br#"<?xml version="1.0"?><worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Inline</t></is></c></row></sheetData></worksheet>"#,
        );
        let text = extract_xlsx_text(&bytes, "inline.xlsx").unwrap();
        assert!(text.contains("Inline"));
    }

    fn minimal_xlsx_bytes(shared_strings_xml: &[u8], sheet_xml: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buf));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file("xl/sharedStrings.xml", options).unwrap();
            zip.write_all(shared_strings_xml).unwrap();
            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(sheet_xml).unwrap();
            zip.start_file("xl/workbook.xml", options).unwrap();
            zip.write_all(b"<workbook/>").unwrap();
            zip.finish().unwrap();
        }
        buf
    }
}
