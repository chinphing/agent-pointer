//! Hybrid lint tool: built-in stacks for common languages (see bundled tool prompt) plus optional `.pointer/lint.toml`.

mod config;
mod detect;
mod exec;
mod parsers;

use crate::tools::file::{resolve_tool_workspace_root, resolve_within_workspace_root};
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use config::{load_lint_config, validate_parser, LintCommandEntry};
use detect::BuiltinStack;
use log::{info, warn};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const READ_LINTS_DEFAULT_WALL_MS: u64 = 120_000;
const READ_LINTS_MAX_WALL_MS: u64 = 300_000;
const READ_LINTS_MAX_DIAGNOSTICS: usize = 200;
const READ_LINTS_STDERR_TAIL: usize = 8_192;
const READ_LINTS_MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
const READ_LINTS_MIN_SLICE_MS: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackMode {
    Auto,
    ConfigOnly,
    /// `stack: "java"` — pick Maven or Gradle from the workspace root.
    JavaAuto,
    Single(BuiltinStack),
}

fn parse_stack_mode(args: &Value) -> StackMode {
    let s = args
        .get("stack")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("auto")
        .to_ascii_lowercase();
    match s.as_str() {
        "config" | "config-only" => StackMode::ConfigOnly,
        "rust" | "cargo" | "clippy" => StackMode::Single(BuiltinStack::RustClippy),
        "node" | "eslint" | "js" | "ts" => StackMode::Single(BuiltinStack::NodeEslint),
        "oxlint" => StackMode::Single(BuiltinStack::NodeOxlint),
        "python" | "ruff" | "py" => StackMode::Single(BuiltinStack::PythonRuff),
        "go" | "golangci" => StackMode::Single(BuiltinStack::GoGolangci),
        "java" | "jvm" => StackMode::JavaAuto,
        "maven" | "java-maven" | "mvnd" => StackMode::Single(BuiltinStack::JavaMaven),
        "gradle" | "java-gradle" => StackMode::Single(BuiltinStack::JavaGradle),
        "dotnet" | "csharp" | "cs" => StackMode::Single(BuiltinStack::Dotnet),
        "php" | "phpstan" => StackMode::Single(BuiltinStack::PhpStan),
        "ruby" | "rubocop" => StackMode::Single(BuiltinStack::Rubocop),
        "swift" | "spm" => StackMode::Single(BuiltinStack::SwiftPm),
        "dart" | "flutter" => StackMode::Single(BuiltinStack::DartAnalyze),
        "all" | "auto" | "" => StackMode::Auto,
        _ => StackMode::Auto,
    }
}

fn stack_mode_json(mode: &StackMode) -> Value {
    match mode {
        StackMode::Auto => json!("auto"),
        StackMode::ConfigOnly => json!("config"),
        StackMode::JavaAuto => json!("java"),
        StackMode::Single(BuiltinStack::RustClippy) => json!("rust"),
        StackMode::Single(BuiltinStack::NodeEslint) => json!("node"),
        StackMode::Single(BuiltinStack::NodeOxlint) => json!("oxlint"),
        StackMode::Single(BuiltinStack::PythonRuff) => json!("python"),
        StackMode::Single(BuiltinStack::GoGolangci) => json!("go"),
        StackMode::Single(BuiltinStack::JavaMaven) => json!("java-maven"),
        StackMode::Single(BuiltinStack::JavaGradle) => json!("java-gradle"),
        StackMode::Single(BuiltinStack::Dotnet) => json!("dotnet"),
        StackMode::Single(BuiltinStack::PhpStan) => json!("php"),
        StackMode::Single(BuiltinStack::Rubocop) => json!("ruby"),
        StackMode::Single(BuiltinStack::SwiftPm) => json!("swift"),
        StackMode::Single(BuiltinStack::DartAnalyze) => json!("dart"),
    }
}

fn wall_ms_from_args(args: &Value) -> u64 {
    args.get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(READ_LINTS_DEFAULT_WALL_MS)
        .clamp(5_000, READ_LINTS_MAX_WALL_MS)
}

fn slice_wall_ms(total_wall_ms: u64, run_count: usize) -> u64 {
    let n = run_count.max(1) as u64;
    (total_wall_ms / n).max(READ_LINTS_MIN_SLICE_MS).min(total_wall_ms)
}

fn parse_paths_filter(root: &Path, args: &Value) -> Result<Vec<PathBuf>> {
    let Some(arr) = args.get("paths").and_then(|p| p.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in arr {
        let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        match resolve_within_workspace_root(root, s) {
            Ok(p) => out.push(p),
            Err(e) => warn!("read_lints: skip invalid filter path {s:?}: {e}"),
        }
    }
    Ok(out)
}

fn path_is_under_workspace(candidate: &Path, workspace: &Path) -> bool {
    let c = candidate.canonicalize().unwrap_or_else(|_| candidate.to_path_buf());
    let w = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    c.starts_with(&w)
}

fn diagnostic_matches_path_filters(file_path: &str, filters: &[PathBuf]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let p = Path::new(file_path);
    let c = match p.canonicalize() {
        Ok(x) => x,
        Err(_) => p.to_path_buf(),
    };
    filters.iter().any(|f| c == *f || c.starts_with(f))
}

fn tail_bytes(raw: &[u8], max: usize) -> String {
    let start = raw.len().saturating_sub(max);
    String::from_utf8_lossy(&raw[start..]).into_owned()
}

fn push_filtered_batch(
    root: &Path,
    filters: &[PathBuf],
    batch: Vec<Value>,
    diagnostics: &mut Vec<Value>,
    truncated: &mut bool,
) {
    for d in batch {
        if diagnostics.len() >= READ_LINTS_MAX_DIAGNOSTICS {
            *truncated = true;
            return;
        }
        let path = d.get("path").and_then(|p| p.as_str()).unwrap_or("");
        if path.is_empty() {
            continue;
        }
        if !path_is_under_workspace(Path::new(path), root) {
            continue;
        }
        if !diagnostic_matches_path_filters(path, filters) {
            continue;
        }
        diagnostics.push(d);
    }
}

#[derive(Debug, Clone)]
enum RunPlan {
    Builtin(BuiltinStack),
    /// Built-in stack with a package/project cwd (monorepo subfolder).
    BuiltinInDir {
        stack: BuiltinStack,
        dir: PathBuf,
    },
    Config(LintCommandEntry),
}

fn append_node_linter_plans_from_filters(
    root: &Path,
    filters: &[PathBuf],
    stack: BuiltinStack,
    detect_roots: fn(&Path, &[PathBuf]) -> Vec<PathBuf>,
    plans: &mut Vec<RunPlan>,
) {
    if filters.is_empty() {
        return;
    }
    let has_stack = plans.iter().any(|p| match p {
        RunPlan::Builtin(s) => *s == stack,
        RunPlan::BuiltinInDir { stack: s, .. } => *s == stack,
        _ => false,
    });
    if has_stack {
        return;
    }
    for dir in detect_roots(root, filters) {
        plans.push(RunPlan::BuiltinInDir { stack, dir });
    }
}

fn append_eslint_plans_from_filters(
    root: &Path,
    filters: &[PathBuf],
    plans: &mut Vec<RunPlan>,
) {
    append_node_linter_plans_from_filters(
        root,
        filters,
        BuiltinStack::NodeEslint,
        detect::eslint_project_roots_from_filters,
        plans,
    );
}

fn append_oxlint_plans_from_filters(
    root: &Path,
    filters: &[PathBuf],
    plans: &mut Vec<RunPlan>,
) {
    append_node_linter_plans_from_filters(
        root,
        filters,
        BuiltinStack::NodeOxlint,
        detect::oxlint_project_roots_from_filters,
        plans,
    );
}

fn monorepo_fallback_plans(
    root: &Path,
    filters: &[PathBuf],
    stack: BuiltinStack,
    detect_roots: fn(&Path, &[PathBuf]) -> Vec<PathBuf>,
) -> Vec<RunPlan> {
    detect_roots(root, filters)
        .into_iter()
        .map(|dir| RunPlan::BuiltinInDir { stack, dir })
        .collect()
}

fn build_run_plan(root: &Path, mode: StackMode, filters: &[PathBuf]) -> Vec<RunPlan> {
    match mode {
        StackMode::ConfigOnly => load_lint_config(root)
            .map(|f| f.commands.into_iter().map(RunPlan::Config).collect())
            .unwrap_or_default(),
        StackMode::JavaAuto => detect::pick_java_stack(root)
            .into_iter()
            .map(RunPlan::Builtin)
            .collect(),
        StackMode::Single(b) => {
            let ok = match b {
                BuiltinStack::NodeEslint => detect::detect_node_eslint(root),
                BuiltinStack::NodeOxlint => detect::detect_node_oxlint(root),
                BuiltinStack::PythonRuff => detect::detect_python_ruff(root),
                BuiltinStack::RustClippy => detect::detect_rust_clippy(root),
                BuiltinStack::GoGolangci => detect::detect_go(root),
                BuiltinStack::JavaMaven => detect::detect_java_maven(root),
                BuiltinStack::JavaGradle => detect::detect_java_gradle(root),
                BuiltinStack::Dotnet => detect::detect_dotnet(root),
                BuiltinStack::PhpStan => detect::detect_phpstan(root),
                BuiltinStack::Rubocop => detect::detect_rubocop(root),
                BuiltinStack::SwiftPm => detect::detect_swift_pm(root),
                BuiltinStack::DartAnalyze => detect::detect_dart_pub(root),
            };
            if ok {
                vec![RunPlan::Builtin(b)]
            } else if b == BuiltinStack::NodeEslint {
                monorepo_fallback_plans(
                    root,
                    filters,
                    BuiltinStack::NodeEslint,
                    detect::eslint_project_roots_from_filters,
                )
            } else if b == BuiltinStack::NodeOxlint {
                monorepo_fallback_plans(
                    root,
                    filters,
                    BuiltinStack::NodeOxlint,
                    detect::oxlint_project_roots_from_filters,
                )
            } else {
                Vec::new()
            }
        }
        StackMode::Auto => {
            let mut builtins = detect::detect_builtin_stacks(root);
            builtins = detect::filter_stacks_by_paths(builtins, filters);
            let mut v: Vec<RunPlan> = builtins
                .into_iter()
                .map(RunPlan::Builtin)
                .collect();
            append_eslint_plans_from_filters(root, filters, &mut v);
            append_oxlint_plans_from_filters(root, filters, &mut v);
            if let Some(f) = load_lint_config(root) {
                for c in f.commands {
                    v.push(RunPlan::Config(c));
                }
            }
            v
        }
    }
}

fn node_tool_program_and_args(
    cwd: &Path,
    workspace: &Path,
    tool: &str,
    filters: &[PathBuf],
    trailing: &[&str],
) -> (&'static str, Vec<String>) {
    let lock_root = detect::find_node_lock_root(cwd, workspace);
    let mut args: Vec<String> = Vec::new();
    if lock_root.join("pnpm-lock.yaml").is_file() {
        args.push("exec".into());
        args.push(tool.into());
    } else if lock_root.join("yarn.lock").is_file() {
        args.push(tool.into());
    } else {
        args.push("--yes".into());
        args.push(tool.into());
    }
    if filters.is_empty() {
        args.push(".".into());
    } else {
        for p in filters {
            args.push(p.display().to_string());
        }
    }
    for t in trailing {
        args.push((*t).into());
    }
    if lock_root.join("pnpm-lock.yaml").is_file() {
        ("pnpm", args)
    } else if lock_root.join("yarn.lock").is_file() {
        ("yarn", args)
    } else {
        ("npx", args)
    }
}

fn eslint_program_and_args(cwd: &Path, workspace: &Path, filters: &[PathBuf]) -> (&'static str, Vec<String>) {
    node_tool_program_and_args(
        cwd,
        workspace,
        "eslint",
        filters,
        &["-f", "json", "--max-warnings", "99999"],
    )
}

fn oxlint_program_and_args(cwd: &Path, workspace: &Path, filters: &[PathBuf]) -> (&'static str, Vec<String>) {
    node_tool_program_and_args(cwd, workspace, "oxlint", filters, &["-f", "json"])
}

fn run_builtin(
    stack: BuiltinStack,
    workspace: &Path,
    cwd: &Path,
    filters: &[PathBuf],
    wall_ms: u64,
) -> (Value, Vec<Value>) {
    let engine_label = match stack {
        BuiltinStack::NodeEslint => "eslint",
        BuiltinStack::NodeOxlint => "oxlint",
        BuiltinStack::PythonRuff => "ruff",
        BuiltinStack::RustClippy => "cargo-clippy",
        BuiltinStack::GoGolangci => "golangci-lint",
        BuiltinStack::JavaMaven => "java-maven",
        BuiltinStack::JavaGradle => "java-gradle",
        BuiltinStack::Dotnet => "dotnet",
        BuiltinStack::PhpStan => "phpstan",
        BuiltinStack::Rubocop => "rubocop",
        BuiltinStack::SwiftPm => "swift",
        BuiltinStack::DartAnalyze => "dart-analyze",
    };
    let source_tag = format!("read_lints:{engine_label}");

    let res = match stack {
        BuiltinStack::NodeEslint => {
            run_eslint_builtin(cwd, workspace, filters, wall_ms, &source_tag)
        }
        BuiltinStack::NodeOxlint => {
            run_oxlint_builtin(cwd, workspace, filters, wall_ms, &source_tag)
        }
        BuiltinStack::PythonRuff => run_ruff_builtin(cwd, filters, wall_ms, &source_tag),
        BuiltinStack::RustClippy => run_clippy_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::GoGolangci => run_go_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::JavaMaven => run_java_maven_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::JavaGradle => run_java_gradle_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::Dotnet => run_dotnet_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::PhpStan => run_phpstan_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::Rubocop => run_rubocop_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::SwiftPm => run_swift_pm_builtin(cwd, wall_ms, &source_tag),
        BuiltinStack::DartAnalyze => run_dart_analyze_builtin(cwd, wall_ms, &source_tag),
    };

    match res {
        Ok((diags, cap)) => {
            let mut meta = json!({
                "engine": engine_label,
                "exitCode": cap.exit_code,
                "timedOut": cap.timed_out,
                "stdoutTruncated": cap.stdout_truncated,
                "stderrTruncated": cap.stderr_truncated,
                "stderrTailUtf8": tail_bytes(&cap.stderr, READ_LINTS_STDERR_TAIL),
                "diagnosticCount": diags.len(),
            });
            if cwd != workspace {
                meta["cwd"] = json!(cwd.display().to_string());
            }
            (meta, diags)
        }
        Err(e) => {
            warn!("read_lints: {} failed: {e}", engine_label);
            let meta = json!({
                "engine": engine_label,
                "skipped": true,
                "reason": e.to_string(),
            });
            (meta, Vec::new())
        }
    }
}

fn run_eslint_builtin(
    cwd: &Path,
    workspace: &Path,
    filters: &[PathBuf],
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let (prog, args) = eslint_program_and_args(cwd, workspace, filters);
    let args: Vec<String> = args;
    let (_lines_unused, cap) = exec::run_argv_capture_lines(
        prog,
        &args,
        cwd,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let stdout = String::from_utf8_lossy(&cap.stdout);
    let diags = parsers::parse_eslint_json(&stdout, source).unwrap_or_else(|_| {
        parsers::text_on_failure_diagnostic(
            &cwd.display().to_string(),
            &format!("{prog} {}", args.join(" ")),
            &cap,
            source,
        )
    });
    Ok((diags, cap))
}

fn run_oxlint_builtin(
    cwd: &Path,
    workspace: &Path,
    filters: &[PathBuf],
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let (prog, args) = oxlint_program_and_args(cwd, workspace, filters);
    let args: Vec<String> = args;
    let (_lines_unused, cap) = exec::run_argv_capture_lines(
        prog,
        &args,
        cwd,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let stdout = String::from_utf8_lossy(&cap.stdout);
    let diags = parsers::parse_oxlint_json(&stdout, source).unwrap_or_else(|_| {
        parsers::text_on_failure_diagnostic(
            &cwd.display().to_string(),
            &format!("{prog} {}", args.join(" ")),
            &cap,
            source,
        )
    });
    Ok((diags, cap))
}

fn run_indicates_tool_failure(run: &Value) -> bool {
    if run.get("skipped").and_then(|s| s.as_bool()).unwrap_or(false) {
        return false;
    }
    if run.get("timedOut").and_then(|t| t.as_bool()).unwrap_or(false) {
        return true;
    }
    let Some(exit) = run.get("exitCode").and_then(|c| c.as_u64()) else {
        return false;
    };
    if exit == 0 {
        return false;
    }
    let stderr = run
        .get("stderrTailUtf8")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .unwrap_or("");
    !stderr.is_empty()
}

fn summarize_lint_result(
    runs: &[Value],
    diagnostics: &[Value],
    diagnostics_truncated: bool,
) -> (bool, &'static str, &'static str) {
    let executed: Vec<&Value> = runs
        .iter()
        .filter(|r| !r.get("skipped").and_then(|s| s.as_bool()).unwrap_or(false))
        .collect();
    if executed.is_empty() {
        if runs.is_empty() {
            return (
                false,
                "skipped_no_matching_stack",
                "No linter ran: no matching stack for this workspace (see hint).",
            );
        }
        return (
            false,
            "skipped_all_runs_failed",
            "No linter ran: every planned run was skipped or failed to start.",
        );
    }
    if !diagnostics.is_empty() {
        let msg = if diagnostics_truncated {
            "Static check ran; issues found (diagnostics list truncated)."
        } else {
            "Static check ran; issues found."
        };
        return (true, "issues_found", msg);
    }
    if executed.iter().any(|r| run_indicates_tool_failure(r)) {
        return (
            true,
            "tool_failed",
            "Static check failed: linter command exited with errors (see runs.stderrTailUtf8 and exitCode).",
        );
    }
    (
        true,
        "clean",
        "Static check ran; no issues reported for the requested scope.",
    )
}

fn run_ruff_builtin(
    root: &Path,
    filters: &[PathBuf],
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let mut args = vec![
        "check".into(),
        "--output-format".into(),
        "json".into(),
    ];
    if filters.is_empty() {
        args.push(".".into());
    } else {
        for p in filters {
            args.push(p.display().to_string());
        }
    }
    let (_lines, cap) = exec::run_argv_capture_lines(
        "ruff",
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let stdout = String::from_utf8_lossy(&cap.stdout);
    let diags = parsers::parse_ruff_json(&stdout, source).unwrap_or_else(|_| {
        parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("ruff {}", args.join(" ")),
            &cap,
            source,
        )
    });
    Ok((diags, cap))
}

fn run_clippy_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let args = vec![
        "clippy".into(),
        "--all-targets".into(),
        "--message-format=json".into(),
    ];
    let (lines, cap) = exec::run_argv_capture_lines(
        "cargo",
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let diags = parsers::parse_cargo_compiler_messages(&lines, source);
    Ok((diags, cap))
}

fn run_go_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let args = vec![
        "run".into(),
        "--out-format=json".into(),
        "./...".into(),
    ];
    let (lines, cap) = exec::run_argv_capture_lines(
        "golangci-lint",
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let stdout = lines.join("\n");
    let diags = parsers::parse_golangci_json(&stdout, source).unwrap_or_else(|_| {
        parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("golangci-lint {}", args.join(" ")),
            &cap,
            source,
        )
    });
    Ok((diags, cap))
}

fn maven_program(root: &Path) -> String {
    #[cfg(windows)]
    {
        if root.join("mvnw.cmd").is_file() {
            return "mvnw.cmd".to_string();
        }
    }
    #[cfg(not(windows))]
    {
        if root.join("mvnw").is_file() {
            return "./mvnw".to_string();
        }
    }
    "mvn".to_string()
}

fn gradle_program(root: &Path) -> String {
    #[cfg(windows)]
    {
        if root.join("gradlew.bat").is_file() {
            return "gradlew.bat".to_string();
        }
    }
    #[cfg(not(windows))]
    {
        if root.join("gradlew").is_file() {
            return "./gradlew".to_string();
        }
    }
    "gradle".to_string()
}

fn combine_process_output(cap: &exec::CapturedOutput) -> String {
    let mut s = String::from_utf8_lossy(&cap.stdout).into_owned();
    s.push('\n');
    s.push_str(&String::from_utf8_lossy(&cap.stderr));
    s
}

fn run_java_maven_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let prog = maven_program(root);
    let args = vec![
        "--batch-mode".into(),
        "-q".into(),
        "-DskipTests".into(),
        "compile".into(),
    ];
    let (_lines, cap) = exec::run_argv_capture_lines(
        &prog,
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let combined = combine_process_output(&cap);
    let mut diags = parsers::parse_java_compile_output(&combined, source);
    if diags.is_empty() && (cap.exit_code != Some(0) || cap.timed_out) {
        diags = parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("{prog} {}", args.join(" ")),
            &cap,
            source,
        );
    }
    Ok((diags, cap))
}

fn run_java_gradle_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let prog = gradle_program(root);
    let args = vec![
        "compileJava".into(),
        "--console=plain".into(),
    ];
    let (_lines, cap) = exec::run_argv_capture_lines(
        &prog,
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let combined = combine_process_output(&cap);
    let mut diags = parsers::parse_java_compile_output(&combined, source);
    if diags.is_empty() && (cap.exit_code != Some(0) || cap.timed_out) {
        diags = parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("{prog} {}", args.join(" ")),
            &cap,
            source,
        );
    }
    Ok((diags, cap))
}

fn dotnet_target_arg(root: &Path) -> Option<String> {
    let mut slns: Vec<String> = Vec::new();
    let mut csprojs: Vec<String> = Vec::new();
    let rd = root.read_dir().ok()?;
    for e in rd.flatten() {
        let p = e.path();
        let (Some(ext), Some(fname)) = (p.extension().and_then(|x| x.to_str()), p.file_name()) else {
            continue;
        };
        let name = fname.to_string_lossy().into_owned();
        if ext.eq_ignore_ascii_case("sln") {
            slns.push(name);
        } else if ext.eq_ignore_ascii_case("csproj") {
            csprojs.push(name);
        }
    }
    slns.sort();
    csprojs.sort();
    slns.into_iter().next().or_else(|| csprojs.into_iter().next())
}

fn run_dotnet_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let Some(target) = dotnet_target_arg(root) else {
        return Err(anyhow!("未在仓库根目录找到 .sln 或 .csproj"));
    };
    let args = vec![
        "build".into(),
        target,
        "-v:q".into(),
        "--nologo".into(),
    ];
    let (_lines, cap) = exec::run_argv_capture_lines(
        "dotnet",
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let combined = combine_process_output(&cap);
    let mut diags = parsers::parse_dotnet_build_log(&combined, source);
    if diags.is_empty() && (cap.exit_code != Some(0) || cap.timed_out) {
        diags = parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("dotnet {}", args.join(" ")),
            &cap,
            source,
        );
    }
    Ok((diags, cap))
}

fn run_phpstan_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let args = vec![
        "exec".into(),
        "phpstan".into(),
        "analyse".into(),
        "--no-progress".into(),
        "--error-format=json".into(),
        ".".into(),
    ];
    let (_lines, cap) = exec::run_argv_capture_lines(
        "composer",
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let stdout = String::from_utf8_lossy(&cap.stdout);
    let mut diags = parsers::parse_phpstan_json(&stdout, root, source).unwrap_or_default();
    if diags.is_empty() && (cap.exit_code != Some(0) || cap.timed_out) {
        diags = parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("composer {}", args.join(" ")),
            &cap,
            source,
        );
    }
    Ok((diags, cap))
}

fn run_rubocop_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let (prog, args): (&str, Vec<String>) = if root.join("Gemfile").is_file() {
        (
            "bundle",
            vec![
                "exec".into(),
                "rubocop".into(),
                "--format".into(),
                "json".into(),
                ".".into(),
            ],
        )
    } else {
        (
            "rubocop",
            vec!["--format".into(), "json".into(), ".".into()],
        )
    };
    let (_lines, cap) = exec::run_argv_capture_lines(
        prog,
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let stdout = String::from_utf8_lossy(&cap.stdout);
    let mut diags = parsers::parse_rubocop_json(&stdout, root, source).unwrap_or_default();
    if diags.is_empty() && (cap.exit_code != Some(0) || cap.timed_out) {
        diags = parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("{prog} {}", args.join(" ")),
            &cap,
            source,
        );
    }
    Ok((diags, cap))
}

fn run_swift_pm_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let args = vec!["build".into()];
    let (_lines, cap) = exec::run_argv_capture_lines(
        "swift",
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let combined = combine_process_output(&cap);
    let mut diags = parsers::parse_swift_build_log(&combined, source);
    if diags.is_empty() && (cap.exit_code != Some(0) || cap.timed_out) {
        diags = parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("swift {}", args.join(" ")),
            &cap,
            source,
        );
    }
    Ok((diags, cap))
}

fn run_dart_analyze_builtin(
    root: &Path,
    wall_ms: u64,
    source: &str,
) -> Result<(Vec<Value>, exec::CapturedOutput)> {
    let args = vec!["analyze".into(), "--format=json".into()];
    let (_lines, cap) = exec::run_argv_capture_lines(
        "dart",
        &args,
        root,
        wall_ms,
        READ_LINTS_MAX_STREAM_BYTES,
    )?;
    let stdout = String::from_utf8_lossy(&cap.stdout);
    let mut diags = parsers::parse_dart_analyze_json(&stdout, source).unwrap_or_default();
    if diags.is_empty() && (cap.exit_code != Some(0) || cap.timed_out) {
        diags = parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &format!("dart {}", args.join(" ")),
            &cap,
            source,
        );
    }
    Ok((diags, cap))
}

fn run_config_entry(
    root: &Path,
    entry: &LintCommandEntry,
    wall_ms: u64,
) -> Result<(Value, Vec<Value>)> {
    validate_parser(entry.parser.trim())?;
    let parser = entry.parser.trim();
    let source = format!("read_lints:config:{parser}");
    let cap = exec::run_shell_capture(root, &entry.shell, wall_ms, READ_LINTS_MAX_STREAM_BYTES)?;
    let stdout = String::from_utf8_lossy(&cap.stdout).into_owned();
    let lines: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
    let diags = match parser {
        "eslint-json" => parsers::parse_eslint_json(&stdout, &source).unwrap_or_else(|_| {
            parsers::text_on_failure_diagnostic(
                &root.display().to_string(),
                &entry.shell,
                &cap,
                &source,
            )
        }),
        "oxlint-json" => parsers::parse_oxlint_json(&stdout, &source).unwrap_or_else(|_| {
            parsers::text_on_failure_diagnostic(
                &root.display().to_string(),
                &entry.shell,
                &cap,
                &source,
            )
        }),
        "ruff-json" => parsers::parse_ruff_json(&stdout, &source).unwrap_or_else(|_| {
            parsers::text_on_failure_diagnostic(
                &root.display().to_string(),
                &entry.shell,
                &cap,
                &source,
            )
        }),
        "cargo-json-lines" => parsers::parse_cargo_compiler_messages(&lines, &source),
        "maven-log" => {
            let mut combined = stdout.clone();
            combined.push('\n');
            combined.push_str(&String::from_utf8_lossy(&cap.stderr));
            parsers::parse_java_compile_output(&combined, &source)
        }
        "text-on-failure" | _ => parsers::text_on_failure_diagnostic(
            &root.display().to_string(),
            &entry.shell,
            &cap,
            &source,
        ),
    };
    let meta = json!({
        "engine": "pointer-lint-config",
        "shell": entry.shell,
        "parser": parser,
        "exitCode": cap.exit_code,
        "timedOut": cap.timed_out,
        "stdoutTruncated": cap.stdout_truncated,
        "stderrTruncated": cap.stderr_truncated,
        "stderrTailUtf8": tail_bytes(&cap.stderr, READ_LINTS_STDERR_TAIL),
        "diagnosticCount": diags.len(),
    });
    Ok((meta, diags))
}

fn run_read_lints(args: Value) -> Result<String> {
    let root = resolve_tool_workspace_root()?;
    let mode = parse_stack_mode(&args);
    let wall_total = wall_ms_from_args(&args);
    let filters = parse_paths_filter(&root, &args)?;
    let plans = build_run_plan(&root, mode, &filters);
    if plans.is_empty() {
        let hint = match mode {
            StackMode::ConfigOnly => "Add `.pointer/lint.toml` with [[commands]] or use stack \"auto\".",
            StackMode::JavaAuto => "No Java project at workspace root (add pom.xml or build.gradle / build.gradle.kts).",
            _ => "No matching linters for this workspace (see built-in stacks in docs/guides/pointer-lint-config.md, or add `.pointer/lint.toml` / use `terminal`). For JS/TS monorepos, ensure `paths` points under a package with ESLint or Oxlint.",
        };
        info!("read_lints: no runs planned ({:?})", mode);
        return Ok(json!({
            "ok": true,
            "lintExecuted": false,
            "outcome": "skipped_no_matching_stack",
            "summary": "No linter ran: no matching stack for this workspace (see hint).",
            "stackMode": stack_mode_json(&mode),
            "workspaceRoot": root.display().to_string(),
            "runs": [],
            "diagnostics": [],
            "hint": hint,
        })
        .to_string());
    }

    let slice = slice_wall_ms(wall_total, plans.len());
    info!(
        "read_lints: {} run(s), {} ms each (budget {} ms), filters={}",
        plans.len(),
        slice,
        wall_total,
        filters.len()
    );

    let mut runs: Vec<Value> = Vec::new();
    let mut diagnostics: Vec<Value> = Vec::new();
    let mut truncated = false;

    for plan in plans {
        if diagnostics.len() >= READ_LINTS_MAX_DIAGNOSTICS {
            truncated = true;
            break;
        }
        match plan {
            RunPlan::Builtin(b) => {
                let (meta, diags) = run_builtin(b, &root, &root, &filters, slice);
                push_filtered_batch(&root, &filters, diags, &mut diagnostics, &mut truncated);
                runs.push(meta);
            }
            RunPlan::BuiltinInDir { stack, dir } => {
                let (meta, diags) = run_builtin(stack, &root, &dir, &filters, slice);
                push_filtered_batch(&root, &filters, diags, &mut diagnostics, &mut truncated);
                runs.push(meta);
            }
            RunPlan::Config(c) => match run_config_entry(&root, &c, slice) {
                Ok((meta, diags)) => {
                    push_filtered_batch(&root, &filters, diags, &mut diagnostics, &mut truncated);
                    runs.push(meta);
                }
                Err(e) => {
                    warn!("read_lints: config command failed: {e}");
                    runs.push(json!({
                        "engine": "pointer-lint-config",
                        "shell": c.shell,
                        "skipped": true,
                        "reason": e.to_string(),
                    }));
                }
            },
        }
    }

    let (lint_executed, outcome, summary) =
        summarize_lint_result(&runs, &diagnostics, truncated);

    Ok(json!({
        "ok": true,
        "lintExecuted": lint_executed,
        "outcome": outcome,
        "summary": summary,
        "stackMode": stack_mode_json(&mode),
        "workspaceRoot": root.display().to_string(),
        "timeoutMsBudget": wall_total,
        "perRunWallMs": slice,
        "runs": runs,
        "diagnostics": diagnostics,
        "diagnosticsTruncated": truncated,
        "diagnosticsCap": READ_LINTS_MAX_DIAGNOSTICS,
    })
    .to_string())
}

pub fn register_all(reg: &ToolRegistry) {
    let doc = include_str!("../prompts/read_lints.md").trim();
    let h: ToolHandler = Arc::new(run_read_lints);
    reg.register(ToolEntry::new(
        "read_lints",
        "low",
        false,
        doc,
        h,
    ));
}

#[cfg(test)]
mod tests {
    use super::detect;
    use super::parsers;
    use super::summarize_lint_result;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn summarize_no_runs_is_not_clean() {
        let (exec, outcome, _) = summarize_lint_result(&[], &[], false);
        assert!(!exec);
        assert_eq!(outcome, "skipped_no_matching_stack");
    }

    #[test]
    fn summarize_clean_when_executed_no_diagnostics() {
        let runs = vec![json!({"engine": "eslint", "exitCode": 0})];
        let (exec, outcome, _) = summarize_lint_result(&runs, &[], false);
        assert!(exec);
        assert_eq!(outcome, "clean");
    }

    #[test]
    fn summarize_issues_when_diagnostics_present() {
        let runs = vec![json!({"engine": "eslint", "exitCode": 1})];
        let diags = vec![json!({"severity": "error", "message": "x"})];
        let (exec, outcome, _) = summarize_lint_result(&runs, &diags, false);
        assert!(exec);
        assert_eq!(outcome, "issues_found");
    }

    #[test]
    fn summarize_tool_failed_when_nonzero_exit_with_stderr_and_no_diagnostics() {
        let runs = vec![json!({
            "engine": "cargo-clippy",
            "exitCode": 1,
            "stderrTailUtf8": "error: 'cargo-clippy' is not installed\n",
            "diagnosticCount": 0,
        })];
        let (exec, outcome, summary) = summarize_lint_result(&runs, &[], false);
        assert!(exec);
        assert_eq!(outcome, "tool_failed");
        assert!(summary.contains("failed"));
    }

    #[test]
    fn summarize_tool_failed_when_timed_out() {
        let runs = vec![json!({
            "engine": "eslint",
            "exitCode": 1,
            "timedOut": true,
        })];
        let (exec, outcome, _) = summarize_lint_result(&runs, &[], false);
        assert!(exec);
        assert_eq!(outcome, "tool_failed");
    }

    #[test]
    fn eslint_project_roots_from_filters_finds_package_from_nested_file() {
        let ws = tempdir().unwrap();
        let pkg = ws.path().join("apps/web");
        fs::create_dir_all(pkg.join("src")).unwrap();
        fs::write(
            pkg.join("package.json"),
            r#"{"devDependencies":{"eslint":"^9.0.0"}}"#,
        )
        .unwrap();
        fs::write(pkg.join("eslint.config.js"), "export default [];\n").unwrap();
        let vue = pkg.join("src/App.vue");
        fs::write(&vue, "<template></template>\n").unwrap();
        let filters = vec![vue.canonicalize().unwrap()];
        let roots = detect::eslint_project_roots_from_filters(ws.path(), &filters);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], pkg.canonicalize().unwrap());
    }

    #[test]
    fn eslint_project_roots_from_filters_dedupes() {
        let ws = tempdir().unwrap();
        let pkg = ws.path().join("pkg");
        fs::create_dir_all(pkg.join("a")).unwrap();
        fs::create_dir_all(pkg.join("b")).unwrap();
        fs::write(
            pkg.join("package.json"),
            r#"{"devDependencies":{"eslint":"^9.0.0"}}"#,
        )
        .unwrap();
        fs::write(pkg.join("eslint.config.js"), "export default [];\n").unwrap();
        let a = pkg.join("a/x.ts");
        let b = pkg.join("b/y.ts");
        fs::write(&a, "").unwrap();
        fs::write(&b, "").unwrap();
        let filters = vec![a.canonicalize().unwrap(), b.canonicalize().unwrap()];
        let roots = detect::eslint_project_roots_from_filters(ws.path(), &filters);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], pkg.canonicalize().unwrap());
    }

    #[test]
    fn oxlint_project_roots_from_filters_finds_package_from_nested_file() {
        let ws = tempdir().unwrap();
        let pkg = ws.path().join("apps/web");
        fs::create_dir_all(pkg.join("src")).unwrap();
        fs::write(
            pkg.join("package.json"),
            r#"{"devDependencies":{"oxlint":"^1.0.0"}}"#,
        )
        .unwrap();
        fs::write(pkg.join(".oxlintrc.json"), "{}\n").unwrap();
        let vue = pkg.join("src/App.vue");
        fs::write(&vue, "<template></template>\n").unwrap();
        let filters = vec![vue.canonicalize().unwrap()];
        let roots = detect::oxlint_project_roots_from_filters(ws.path(), &filters);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], pkg.canonicalize().unwrap());
    }

    #[test]
    fn oxlint_json_parses_diagnostic() {
        let raw = r#"{"diagnostics":[{"message":"bad","code":"eslint/no-debugger","severity":"error","filename":"/w/a.ts","labels":[{"span":{"line":2,"column":3}}]}]}"#;
        let ds = parsers::parse_oxlint_json(raw, "oxlint").unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["severity"], "error");
        assert_eq!(ds[0]["line"], 2);
        assert_eq!(ds[0]["column"], 3);
    }

    #[test]
    fn eslint_json_parses_messages() {
        let raw = r#"[{"filePath":"/w/a.ts","messages":[{"severity":2,"message":"no-undef","line":1,"column":1,"ruleId":"x"}]}]"#;
        let ds = parsers::parse_eslint_json(raw, "eslint").unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["severity"], "error");
        assert_eq!(ds[0]["code"], "x");
    }

    #[test]
    fn compiler_message_to_diagnostic_parses_span() {
        let msg: serde_json::Value = serde_json::from_str(
            r#"{"message":"unused variable","code":{"code":"unused_variables"},"level":"warning","spans":[{"file_name":"D:/proj/src/lib.rs","line_start":3,"column_start":9,"is_primary":true}],"rendered":"warning: unused variable\n --> src/lib.rs:3:9\n"}"#,
        )
        .unwrap();
        let lines = vec![format!(
            r#"{{"reason":"compiler-message","message":{}}}"#,
            serde_json::to_string(&msg).unwrap()
        )];
        let ds = parsers::parse_cargo_compiler_messages(&lines, "cargo-clippy");
        let d = &ds[0];
        assert_eq!(d["severity"], "warning");
        assert_eq!(d["line"], 3);
        assert_eq!(d["column"], 9);
        assert_eq!(d["code"], "unused_variables");
    }

    #[test]
    fn java_maven_log_parses_error_line() {
        let log = "[ERROR] /workspace/src/Foo.java:[3,10] cannot find symbol\n";
        let ds = parsers::parse_java_compile_output(log, "read_lints:java-maven");
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["line"], 3);
        assert_eq!(ds[0]["column"], 10);
        assert_eq!(ds[0]["severity"], "error");
    }

    #[test]
    fn java_javac_style_line_parses() {
        let log = "src/Foo.java:5: error: ';' expected\n";
        let ds = parsers::parse_java_compile_output(log, "read_lints:java-gradle");
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["line"], 5);
    }

    #[test]
    fn dotnet_msbuild_line_parses() {
        let log = "src\\Foo.cs(2,3): error CS1002: ; expected\n";
        let ds = parsers::parse_dotnet_build_log(log, "dotnet");
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["line"], 2);
        assert_eq!(ds[0]["code"], "CS1002");
    }

    #[test]
    fn phpstan_json_resolves_relative_paths() {
        let j = r#"{"files":{"app/Foo.php":{"messages":[{"line":3,"message":"x","identifier":"rule"}]}}}"#;
        let ws = std::path::Path::new("ws");
        let ds = parsers::parse_phpstan_json(j, ws, "phpstan").unwrap();
        assert_eq!(ds.len(), 1);
        assert!(ds[0]["path"].as_str().unwrap().contains("Foo.php"));
    }

    #[test]
    fn rubocop_json_relative_path() {
        let j = r#"[{"path":"lib/a.rb","offenses":[{"severity":"convention","message":"m","cop_name":"Style/X","location":{"start_line":1,"start_column":1}}]}]"#;
        let ws = std::path::Path::new("repo");
        let ds = parsers::parse_rubocop_json(j, ws, "rubocop").unwrap();
        assert_eq!(ds.len(), 1);
        assert!(ds[0]["path"].as_str().unwrap().contains("a.rb"));
    }

    #[test]
    fn dart_analyze_minimal() {
        let j = r#"{"diagnostics":[{"severity":"ERROR","problemMessage":"bad","code":"x","location":{"file":"file:///p/a.dart","range":{"start":{"line":0,"character":0}}}}]}"#;
        let ds = parsers::parse_dart_analyze_json(j, "dart").unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["severity"], "error");
    }
}
