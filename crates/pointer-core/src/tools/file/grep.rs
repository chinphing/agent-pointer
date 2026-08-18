use super::json_str;
use super::path::{
    build_glob_set, deduplicate_globs, expand_file_types, path_display_abs, path_error_with_hints,
    resolve_existing_read_path, SKIP_EXT,
};
use super::{FileToolLimits, CONTEXT_LINES, MAX_GREP_FILE_BYTES, MAX_WALK_DEPTH};
use crate::text_util::truncate_bytes;
use anyhow::{anyhow, Result};
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{
    BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkFinish, SinkMatch,
};
use ignore::WalkBuilder;
use log::{info, warn};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn should_skip_grep(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| SKIP_EXT.iter().any(|s| s.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn clip_grep_line(s: &str, max_bytes: usize) -> String {
    truncate_bytes(s, max_bytes)
}

/// One `file:grep` hit in the JSON response (same fields as the legacy walker).
struct GrepJsonSink<'a> {
    path_abs: String,
    results: &'a mut Vec<serde_json::Value>,
    max_results: usize,
    hit_line_max: usize,
    output_max: usize,
    output_bytes: &'a mut usize,
    output_capped: &'a mut bool,
    stanza: Vec<(u64, String)>,
    pending_match_line: Option<u64>,
    pending_match_text: Option<String>,
}

impl GrepJsonSink<'_> {
    fn bytes_to_line(&self, s: &[u8]) -> String {
        clip_grep_line(
            String::from_utf8_lossy(s)
                .trim_end_matches('\n')
                .trim_end_matches('\r'),
            self.hit_line_max,
        )
    }

    fn at_limit(&self) -> bool {
        *self.output_capped || self.results.len() >= self.max_results
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        let Some(match_ln) = self.pending_match_line.take() else {
            self.stanza.clear();
            self.pending_match_text.take();
            return Ok(());
        };
        let match_line = self.pending_match_text.take().unwrap_or_default();
        let mut context = self
            .stanza
            .iter()
            .map(|(n, s)| format!("{n}: {s}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.stanza.clear();
        context = crate::text_util::truncate_bytes(&context, self.hit_line_max.saturating_mul(8));
        if self.at_limit() {
            return Ok(());
        }
        let est = self.path_abs.len() + match_line.len() + context.len() + 64;
        if *self.output_bytes + est > self.output_max {
            let slim_est = self.path_abs.len() + match_line.len() + 64;
            if *self.output_bytes + slim_est > self.output_max {
                *self.output_capped = true;
                return Ok(());
            }
            *self.output_bytes += slim_est;
            self.results.push(serde_json::json!({
                "path": &self.path_abs,
                "line": match_ln,
                "matchLine": match_line,
                "context": "",
            }));
            *self.output_capped = true;
            return Ok(());
        }
        *self.output_bytes += est;
        self.results.push(serde_json::json!({
            "path": &self.path_abs,
            "line": match_ln,
            "matchLine": match_line,
            "context": context,
        }));
        Ok(())
    }
}

impl Sink for GrepJsonSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        if self.pending_match_line.is_some() {
            self.flush()?;
        }
        if self.at_limit() {
            return Ok(false);
        }
        let ln = mat
            .line_number()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "grep: missing line number"))?;
        let text = self.bytes_to_line(mat.bytes());
        self.stanza.push((ln, text.clone()));
        self.pending_match_line = Some(ln);
        self.pending_match_text = Some(text);
        Ok(true)
    }

    fn context(&mut self, _searcher: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, io::Error> {
        if self.at_limit() {
            return Ok(false);
        }
        let ln = ctx
            .line_number()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "grep: missing line number"))?;
        let text = self.bytes_to_line(ctx.bytes());
        self.stanza.push((ln, text));
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> Result<bool, io::Error> {
        self.flush()?;
        Ok(!self.at_limit())
    }

    fn finish(&mut self, _searcher: &Searcher, _finish: &SinkFinish) -> Result<(), io::Error> {
        self.flush()
    }
}

fn grep_one_path_with_searcher(
    searcher: &mut Searcher,
    matcher: &grep_regex::RegexMatcher,
    file_path: &Path,
    results: &mut Vec<serde_json::Value>,
    max_results: usize,
    output_bytes: &mut usize,
    output_capped: &mut bool,
    skipped_large: &mut usize,
    limits: &FileToolLimits,
) -> Result<()> {
    if *output_capped || results.len() >= max_results {
        return Ok(());
    }
    if should_skip_grep(file_path) {
        return Ok(());
    }
    let meta = match fs::metadata(file_path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if meta.len() > MAX_GREP_FILE_BYTES as u64 {
        *skipped_large += 1;
        warn!(
            "file_grep: skip oversized file bytes={} cap={} path={}",
            meta.len(),
            MAX_GREP_FILE_BYTES,
            file_path.display()
        );
        return Ok(());
    }
    let path_abs = path_display_abs(file_path);
    let mut sink = GrepJsonSink {
        path_abs,
        results,
        max_results,
        hit_line_max: limits.line_max_bytes,
        output_max: limits.read_max_bytes,
        output_bytes,
        output_capped,
        stanza: Vec::new(),
        pending_match_line: None,
        pending_match_text: None,
    };
    searcher
        .search_path(matcher, file_path, &mut sink)
        .map_err(|e| anyhow!("grep 搜索失败: {e}"))
}

pub(crate) fn execute_file_grep_payload(args: &serde_json::Value, root: &Path) -> Result<String> {
    execute_file_grep_payload_with(args, root, &FileToolLimits::default())
}

pub(crate) fn execute_file_grep_payload_with(
    args: &serde_json::Value,
    root: &Path,
    limits: &FileToolLimits,
) -> Result<String> {
    let root = root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 pattern"))?;
    if pattern.len() > 512 {
        return Err(anyhow!("正则过长"));
    }
    let ceiling = limits.grep_max_results as u64;
    let requested_max = args.get("maxResults").and_then(|v| v.as_u64());
    if requested_max.is_some_and(|n| n > ceiling) {
        warn!(
            "file_grep: maxResults={} exceeds ceiling {}, clamping",
            requested_max.unwrap(),
            ceiling
        );
    }
    let max_results = requested_max.unwrap_or(ceiling).min(ceiling) as usize;
    let max_depth = args
        .get("maxDepth")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_WALK_DEPTH as u64)
        .min(MAX_WALK_DEPTH as u64) as usize;
    let context = args
        .get("contextLines")
        .and_then(|v| v.as_u64())
        .unwrap_or(CONTEXT_LINES as u64)
        .min(5) as usize;

    // New parameters
    let include_globs: Option<Vec<String>> = args
        .get("includeGlobs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        });
    let exclude_globs: Option<Vec<String>> = args
        .get("excludeGlobs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        });
    let file_types: Option<Vec<String>> =
        args.get("fileTypes").and_then(|v| v.as_array()).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        });
    let fixed_string = args
        .get("fixedString")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ignore_case = args
        .get("ignoreCase")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_hidden = args
        .get("includeHidden")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Expand file types and merge with include_globs
    let merged_include_globs = if let Some(types) = &file_types {
        let mut globs = expand_file_types(types)?;
        if let Some(inc) = &include_globs {
            globs.extend(inc.iter().cloned());
        }
        Some(deduplicate_globs(globs))
    } else {
        include_globs.clone()
    };

    // Build glob sets
    let include_set = build_glob_set(merged_include_globs.as_deref())?;
    let exclude_set = build_glob_set(exclude_globs.as_deref())?;

    let mut matcher_builder = RegexMatcherBuilder::new();
    matcher_builder.multi_line(false);
    if fixed_string {
        matcher_builder.fixed_strings(true);
    }
    if ignore_case {
        matcher_builder.case_insensitive(true);
    }
    let matcher = matcher_builder
        .build(pattern)
        .map_err(|e| anyhow!("正则无效: {e}"))?;

    let mut searcher = SearcherBuilder::new()
        .multi_line(false)
        .before_context(context)
        .after_context(context)
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .heap_limit(Some(MAX_GREP_FILE_BYTES))
        .line_number(true)
        .build();

    if let Some(obj) = args.as_object() {
        if obj.contains_key("subdir") {
            return Err(anyhow!("grep 已移除参数 subdir，请使用 path（文件或目录）"));
        }
    }

    let path_arg = json_str(args, "path", "path")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "缺少 path：file_grep 必须指定搜索范围（已存在的文件或目录），例如 src/ 或 crates/foo/src/lib.rs；\
                 仅在需要全仓搜索时显式传 path: \".\""
            )
        })?;

    enum GrepScope {
        Walk { start: PathBuf, max_depth: usize },
        SingleFile { file: PathBuf },
    }

    let scope = {
        let p = resolve_existing_read_path(&root, path_arg, "grep")?;
        if p.is_dir() {
            GrepScope::Walk {
                start: p,
                max_depth,
            }
        } else if p.is_file() {
            info!("file:grep: single file {}", p.display());
            GrepScope::SingleFile { file: p }
        } else {
            return Err(path_error_with_hints(
                &root,
                path_arg,
                format!("grep path 必须是已存在的文件或目录: {}", p.display()),
            ));
        }
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut output_bytes: usize = 0;
    let mut output_capped = false;
    let mut skipped_large: usize = 0;
    let (root_field, single_file) = match &scope {
        GrepScope::Walk { start, .. } => (path_display_abs(start), false),
        GrepScope::SingleFile { file } => (path_display_abs(file), true),
    };

    match scope {
        GrepScope::SingleFile { file } => {
            grep_one_path_with_searcher(
                &mut searcher,
                &matcher,
                &file,
                &mut results,
                max_results,
                &mut output_bytes,
                &mut output_capped,
                &mut skipped_large,
                limits,
            )?;
        }
        GrepScope::Walk { start, max_depth } => {
            let mut walk = WalkBuilder::new(&start);
            walk.git_ignore(true);
            walk.hidden(!include_hidden);
            walk.max_depth(Some(max_depth));
            walk.filter_entry(|e| !should_skip_grep(e.path()));
            for entry in walk.build() {
                if output_capped || results.len() >= max_results {
                    break;
                }
                let entry = match entry {
                    Ok(e) => e,
                    Err(err) => {
                        warn!("file:grep walk: {err}");
                        continue;
                    }
                };
                if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    continue;
                }
                let p = entry.path();
                // Apply glob filters
                if let Ok(rel) = p.strip_prefix(&start) {
                    if let Some(ref set) = exclude_set {
                        if set.is_match(rel) {
                            continue;
                        }
                    }
                    if let Some(ref set) = include_set {
                        if !set.is_match(rel) {
                            continue;
                        }
                    }
                }
                grep_one_path_with_searcher(
                    &mut searcher,
                    &matcher,
                    p,
                    &mut results,
                    max_results,
                    &mut output_bytes,
                    &mut output_capped,
                    &mut skipped_large,
                    limits,
                )?;
            }
        }
    }

    let truncated = results.len() >= max_results || output_capped;
    if truncated {
        info!(
            "file_grep truncated hits={} output_bytes={} output_capped={} max_results={} skipped_large={}",
            results.len(),
            output_bytes,
            output_capped,
            max_results,
            skipped_large
        );
    }
    let mut out = serde_json::json!({
        "root": root_field,
        "pattern": pattern,
        "results": results,
        "count": results.len(),
        "truncated": truncated,
        "skippedLargeFileCount": skipped_large
    });
    if output_capped {
        out["truncatedByOutputBytes"] = serde_json::json!(true);
    }
    if skipped_large > 0 {
        out["warning"] = serde_json::json!(format!(
            "skipped {skipped_large} file(s) larger than {MAX_GREP_FILE_BYTES} bytes; narrow path or split the file"
        ));
    } else if output_capped {
        out["warning"] = serde_json::json!(
            "grep output hit the hard byte cap; narrow path/pattern — maxResults cannot raise the payload ceiling"
        );
    } else if truncated {
        out["warning"] = serde_json::json!(
            "grep hit the hard result cap; narrow path/pattern — maxResults cannot be raised above the runtime ceiling"
        );
    }
    if single_file {
        out["singleFile"] = serde_json::json!(true);
    }
    Ok(out.to_string())
}
