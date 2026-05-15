//! Normalize linter-specific JSON/text into unified `diagnostics` items.

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

use super::exec::CapturedOutput;

pub fn parse_cargo_compiler_messages(lines: &[String], source: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(msg) = v.get("message") else { continue };
        if let Some(d) = compiler_message_to_diagnostic(msg, source) {
            out.push(d);
        }
    }
    out
}

fn compiler_message_to_diagnostic(msg: &Value, source: &str) -> Option<Value> {
    let level = msg.get("level")?.as_str()?;
    let short = msg.get("message").and_then(|m| m.as_str()).unwrap_or("");
    let rendered = msg.get("rendered").and_then(|m| m.as_str()).unwrap_or(short);
    let code = msg
        .get("code")
        .and_then(|c| c.get("code").and_then(|x| x.as_str()));
    let spans = msg.get("spans")?.as_array()?;
    if spans.is_empty() {
        return None;
    }
    let primary = spans
        .iter()
        .find(|s| s.get("is_primary").and_then(|x| x.as_bool()) == Some(true))
        .or_else(|| spans.first())?;
    let file = primary.get("file_name")?.as_str()?;
    let line = primary.get("line_start")?.as_u64()?;
    let col = primary
        .get("column_start")
        .and_then(|x| x.as_u64())
        .unwrap_or(1);
    Some(json!({
        "severity": level,
        "message": rendered,
        "path": file,
        "line": line,
        "column": col,
        "code": code,
        "source": source
    }))
}

pub fn parse_eslint_json(stdout: &str, source: &str) -> Result<Vec<Value>> {
    let root: Value = serde_json::from_str(stdout).map_err(|e| anyhow!("eslint JSON: {e}"))?;
    let arr = root
        .as_array()
        .ok_or_else(|| anyhow!("eslint JSON: expected array"))?;
    let mut out = Vec::new();
    for file_block in arr {
        let file_path = file_block
            .get("filePath")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let Some(msgs) = file_block.get("messages").and_then(|m| m.as_array()) else {
            continue;
        };
        for m in msgs {
            let sev = m.get("severity").and_then(|x| x.as_u64()).unwrap_or(1);
            let severity = match sev {
                2 => "error",
                1 => "warning",
                _ => "info",
            };
            let message = m
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("eslint");
            let rule = m.get("ruleId").and_then(|x| x.as_str());
            let line = m.get("line").and_then(|x| x.as_u64()).unwrap_or(1);
            let col = m.get("column").and_then(|x| x.as_u64()).unwrap_or(1);
            out.push(json!({
                "severity": severity,
                "message": message,
                "path": file_path,
                "line": line,
                "column": col,
                "code": rule,
                "source": source
            }));
        }
    }
    Ok(out)
}

pub fn parse_oxlint_json(stdout: &str, source: &str) -> Result<Vec<Value>> {
    let root: Value = serde_json::from_str(stdout).map_err(|e| anyhow!("oxlint JSON: {e}"))?;
    let Some(arr) = root.get("diagnostics").and_then(|d| d.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for d in arr {
        let severity = d
            .get("severity")
            .and_then(|x| x.as_str())
            .unwrap_or("warning");
        let message = d
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("oxlint");
        let code = d.get("code").and_then(|x| x.as_str());
        let path = d
            .get("filename")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let (line, col) = d
            .get("labels")
            .and_then(|l| l.as_array())
            .and_then(|labels| labels.first())
            .and_then(|label| label.get("span"))
            .map(|span| {
                (
                    span.get("line").and_then(|x| x.as_u64()).unwrap_or(1),
                    span.get("column").and_then(|x| x.as_u64()).unwrap_or(1),
                )
            })
            .unwrap_or((1, 1));
        out.push(json!({
            "severity": severity,
            "message": message,
            "path": path,
            "line": line,
            "column": col,
            "code": code,
            "source": source
        }));
    }
    Ok(out)
}

pub fn parse_ruff_json(stdout: &str, source: &str) -> Result<Vec<Value>> {
    let root: Value = serde_json::from_str(stdout).map_err(|e| anyhow!("ruff JSON: {e}"))?;
    let arr = root
        .as_array()
        .ok_or_else(|| anyhow!("ruff JSON: expected array"))?;
    let mut out = Vec::new();
    for item in arr {
        let path = item
            .get("filename")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let loc = item.get("location").cloned().unwrap_or(json!({}));
        let line = loc.get("row").and_then(|x| x.as_u64()).unwrap_or(1);
        let col = loc.get("column").and_then(|x| x.as_u64()).unwrap_or(1);
        let message = item
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("ruff");
        let code = item.get("code").and_then(|x| x.as_str());
        let severity = if message.to_ascii_lowercase().contains("error") {
            "error"
        } else {
            "warning"
        };
        out.push(json!({
            "severity": severity,
            "message": message,
            "path": path,
            "line": line,
            "column": col,
            "code": code,
            "source": source
        }));
    }
    Ok(out)
}

pub fn parse_golangci_json(stdout: &str, source: &str) -> Result<Vec<Value>> {
    let root: Value = serde_json::from_str(stdout).map_err(|e| anyhow!("golangci JSON: {e}"))?;
    let issues = root
        .get("Issues")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow!("golangci JSON: missing Issues[]"))?;
    let mut out = Vec::new();
    for iss in issues {
        let text = iss.get("Text").and_then(|x| x.as_str()).unwrap_or("lint");
        let linter = iss.get("FromLinter").and_then(|x| x.as_str()).unwrap_or("golangci-lint");
        let pos = iss.get("Pos").cloned().unwrap_or(json!({}));
        let path = pos.get("Filename").and_then(|x| x.as_str()).unwrap_or("");
        let line = pos.get("Line").and_then(|x| x.as_u64()).unwrap_or(1);
        let col = pos.get("Column").and_then(|x| x.as_u64()).unwrap_or(1);
        let sev = iss
            .get("Severity")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("warning");
        out.push(json!({
            "severity": sev,
            "message": format!("[{linter}] {text}"),
            "path": path,
            "line": line,
            "column": col,
            "code": linter,
            "source": source
        }));
    }
    Ok(out)
}

/// MSBuild / `dotnet build` lines: `path\File.cs(12,3): error CS1002: ; expected`
pub fn parse_dotnet_build_log(combined: &str, source: &str) -> Vec<Value> {
    let re = Regex::new(
        r"(?m)^(?P<file>[^\s].*?\.cs)\((?P<line>\d+),(?P<col>\d+)\):\s*(?P<kind>error|warning)\s+(?P<code>[^\s:]+):\s*(?P<msg>.+)$",
    )
    .expect("valid regex");
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for caps in re.captures_iter(combined) {
        let file = caps.name("file").map(|m| m.as_str().trim()).unwrap_or("");
        let line: u64 = caps
            .name("line")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let col: u64 = caps
            .name("col")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let kind = caps.name("kind").map(|m| m.as_str()).unwrap_or("error");
        let severity = if kind.eq_ignore_ascii_case("warning") {
            "warning"
        } else {
            "error"
        };
        let code = caps.name("code").map(|m| m.as_str()).unwrap_or("dotnet");
        let msg = caps.name("msg").map(|m| m.as_str().trim()).unwrap_or("build");
        let key = format!("{file}|{line}|{col}|{msg}");
        if !seen.insert(key) {
            continue;
        }
        out.push(json!({
            "severity": severity,
            "message": msg,
            "path": file,
            "line": line,
            "column": col,
            "code": code,
            "source": source
        }));
    }
    out
}

pub fn parse_phpstan_json(stdout: &str, workspace_root: &Path, source: &str) -> Result<Vec<Value>> {
    let parsed: Value = serde_json::from_str(stdout).map_err(|e| anyhow!("phpstan JSON: {e}"))?;
    let files = parsed
        .get("files")
        .and_then(|x| x.as_object())
        .ok_or_else(|| anyhow!("phpstan JSON: missing files map"))?;
    let mut out = Vec::new();
    for (rel, finfo) in files {
        let Some(msgs) = finfo.get("messages").and_then(|m| m.as_array()) else {
            continue;
        };
        let full_path = if Path::new(rel).is_absolute() {
            rel.clone()
        } else {
            workspace_root.join(rel).to_string_lossy().into_owned()
        };
        for m in msgs {
            let line = m.get("line").and_then(|x| x.as_u64()).unwrap_or(1);
            let message = m
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("phpstan");
            let ident = m.get("identifier").and_then(|x| x.as_str());
            out.push(json!({
                "severity": "error",
                "message": message,
                "path": full_path.as_str(),
                "line": line,
                "column": 1,
                "code": ident,
                "source": source
            }));
        }
    }
    Ok(out)
}

pub fn parse_rubocop_json(stdout: &str, workspace_root: &Path, source: &str) -> Result<Vec<Value>> {
    let parsed: Value = serde_json::from_str(stdout).map_err(|e| anyhow!("rubocop JSON: {e}"))?;
    let arr = parsed
        .as_array()
        .ok_or_else(|| anyhow!("rubocop JSON: expected top-level array"))?;
    let mut out = Vec::new();
    for file in arr {
        let path_raw = file.get("path").and_then(|x| x.as_str()).unwrap_or("");
        if path_raw.is_empty() {
            continue;
        }
        let path = if Path::new(path_raw).is_absolute() {
            path_raw.to_string()
        } else {
            workspace_root.join(path_raw).to_string_lossy().into_owned()
        };
        let Some(offenses) = file.get("offenses").and_then(|x| x.as_array()) else {
            continue;
        };
        for off in offenses {
            let sev_raw = off.get("severity").and_then(|x| x.as_str()).unwrap_or("convention");
            let severity = match sev_raw {
                "fatal" | "error" => "error",
                _ => "warning",
            };
            let message = off.get("message").and_then(|x| x.as_str()).unwrap_or("rubocop");
            let cop = off.get("cop_name").and_then(|x| x.as_str());
            let loc = off.get("location").cloned().unwrap_or(json!({}));
            let line = loc
                .get("start_line")
                .or_else(|| loc.get("line"))
                .and_then(|x| x.as_u64())
                .unwrap_or(1);
            let col = loc
                .get("start_column")
                .or_else(|| loc.get("column"))
                .and_then(|x| x.as_u64())
                .unwrap_or(1);
            out.push(json!({
                "severity": severity,
                "message": message,
                "path": path.as_str(),
                "line": line,
                "column": col,
                "code": cop,
                "source": source
            }));
        }
    }
    Ok(out)
}

pub fn parse_swift_build_log(combined: &str, source: &str) -> Vec<Value> {
    let re = Regex::new(
        r"(?m)^(?P<file>[^\s].*?\.swift):(?P<line>\d+):(?P<col>\d+):\s*(?P<kind>error|warning):\s*(?P<msg>.+)$",
    )
    .expect("valid regex");
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for caps in re.captures_iter(combined) {
        let file = caps.name("file").map(|m| m.as_str().trim()).unwrap_or("");
        let line: u64 = caps
            .name("line")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let col: u64 = caps
            .name("col")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let kind = caps.name("kind").map(|m| m.as_str()).unwrap_or("error");
        let severity = if kind.eq_ignore_ascii_case("warning") {
            "warning"
        } else {
            "error"
        };
        let msg = caps.name("msg").map(|m| m.as_str().trim()).unwrap_or("swift");
        let key = format!("{file}|{line}|{col}|{msg}");
        if !seen.insert(key) {
            continue;
        }
        out.push(json!({
            "severity": severity,
            "message": msg,
            "path": file,
            "line": line,
            "column": col,
            "code": "swift-build",
            "source": source
        }));
    }
    out
}

fn strip_file_uri(path: &str) -> String {
    path.strip_prefix("file://")
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or(path)
        .replace('\\', "/")
}

pub fn parse_dart_analyze_json(stdout: &str, source: &str) -> Result<Vec<Value>> {
    let root: Value = serde_json::from_str(stdout).map_err(|e| anyhow!("dart analyze JSON: {e}"))?;
    let diagnostics = if let Some(a) = root.get("diagnostics").and_then(|x| x.as_array()) {
        a
    } else if let Some(a) = root.as_array() {
        a
    } else {
        return Err(anyhow!("dart analyze: expected diagnostics array"));
    };
    let mut out = Vec::new();
    for d in diagnostics {
        let sev_raw = d.get("severity").and_then(|x| x.as_str()).unwrap_or("INFO");
        let severity = match sev_raw.to_uppercase().as_str() {
            "ERROR" => "error",
            "WARNING" => "warning",
            _ => "info",
        };
        let message = d.get("problemMessage").or_else(|| d.get("message"));
        let message = message.and_then(|x| x.as_str()).unwrap_or("analyze");
        let code = d.get("code").and_then(|x| x.as_str());
        let loc = d.get("location").cloned().unwrap_or(json!({}));
        let file_raw = loc
            .get("file")
            .and_then(|x| x.as_str())
            .or_else(|| loc.get("uri").and_then(|x| x.as_str()))
            .unwrap_or("");
        let path = strip_file_uri(file_raw);
        let range = loc.get("range").cloned().unwrap_or(json!({}));
        let start = range.get("start").cloned().unwrap_or(json!({}));
        let line = start.get("line").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
        let col = start.get("character").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
        out.push(json!({
            "severity": severity,
            "message": message,
            "path": path,
            "line": line,
            "column": col,
            "code": code,
            "source": source
        }));
    }
    Ok(out)
}

/// Parse Maven (`[ERROR] File.java:[l,c] msg`) and javac-style (`File.java:l: error: msg`) lines.
pub fn parse_java_compile_output(combined: &str, source: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let re_mvn = Regex::new(
        r"(?m)^\[(ERROR|WARNING)\]\s*(?P<file>[^\[\]\r\n]+\.java):\[(?P<line>\d+),(?P<col>\d+)\]\s*(?P<msg>.+)$",
    )
    .expect("valid regex");
    let re_javac = Regex::new(
        r"(?m)^(?P<file>[^\s:]+\.java):(?P<line>\d+):\s*((?P<col>\d+):\s*)?(?P<kind>error|warning|错误|警告):\s*(?P<msg>.+)$",
    )
    .expect("valid regex");

    for caps in re_mvn.captures_iter(combined) {
        let level_raw = caps.get(1).map(|m| m.as_str()).unwrap_or("ERROR");
        let severity = if level_raw.eq_ignore_ascii_case("WARNING") {
            "warning"
        } else {
            "error"
        };
        let file = caps.name("file").map(|m| m.as_str()).unwrap_or("");
        let line: u64 = caps
            .name("line")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let col: u64 = caps
            .name("col")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let msg = caps.name("msg").map(|m| m.as_str().trim()).unwrap_or("compile");
        push_java_diag(&mut out, &mut seen, file, line, col, severity, msg, source, "maven-log");
    }

    for caps in re_javac.captures_iter(combined) {
        let file = caps.name("file").map(|m| m.as_str()).unwrap_or("");
        let line: u64 = caps
            .name("line")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let col: u64 = caps
            .name("col")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let kind = caps.name("kind").map(|m| m.as_str()).unwrap_or("error");
        let severity = if kind.eq_ignore_ascii_case("warning") || kind.contains('告') {
            "warning"
        } else {
            "error"
        };
        let msg = caps.name("msg").map(|m| m.as_str().trim()).unwrap_or("compile");
        push_java_diag(
            &mut out,
            &mut seen,
            file,
            line,
            col,
            severity,
            msg,
            source,
            "javac-log",
        );
    }

    out
}

fn push_java_diag(
    out: &mut Vec<Value>,
    seen: &mut HashSet<String>,
    file: &str,
    line: u64,
    col: u64,
    severity: &str,
    message: &str,
    source: &str,
    code: &str,
) {
    let key = format!("{file}|{line}|{col}|{message}");
    if !seen.insert(key) {
        return;
    }
    out.push(json!({
        "severity": severity,
        "message": message,
        "path": file,
        "line": line,
        "column": col,
        "code": code,
        "source": source
    }));
}

pub fn text_on_failure_diagnostic(
    workspace_display: &str,
    shell: &str,
    cap: &CapturedOutput,
    source: &str,
) -> Vec<Value> {
    if cap.exit_code == Some(0) {
        return Vec::new();
    }
    let mut tail = String::from_utf8_lossy(&cap.stdout).to_string();
    if !cap.stderr.is_empty() {
        tail.push_str("\n--- stderr ---\n");
        tail.push_str(&String::from_utf8_lossy(&cap.stderr));
    }
    let tail: String = tail.chars().rev().take(4000).collect::<String>().chars().rev().collect();
    vec![json!({
        "severity": "error",
        "message": format!("Lint command failed (exit {:?}): {}\n\n{}", cap.exit_code, shell, tail),
        "path": workspace_display,
        "line": 1,
        "column": 1,
        "code": "lint-exit-nonzero",
        "source": source
    })]
}
