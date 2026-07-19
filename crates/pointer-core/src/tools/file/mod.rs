//! Workspace-scoped file tools: single registry tool `file` with `method` (like Computer `mouse:method`).
//! Root from settings `workspaceRoot`, else `current_dir`.
mod edit;
mod glob;
mod grep;
mod list;
mod path;
mod read;
mod write;

use super::{ToolEntry, ToolHandler, ToolRegistry};
use crate::agents::{current_file_tool_lead_profile, AgentProfile};
use anyhow::{anyhow, Result};
use log::warn;
use std::sync::Arc;

pub use path::{
    resolve_accessible_path, resolve_tool_workspace_root, resolve_within_workspace_root,
    resolve_writable_path, workspace_root_from_override_or_settings,
};
pub use path::{AgentWorkspaceGuard, ConversationWorkspaceGuard, set_runtime_workspace_root};

use edit::execute_file_edit_payload;
use glob::execute_file_glob_payload;
use grep::execute_file_grep_payload;
use list::execute_file_list_payload;
use read::execute_file_read;
use write::execute_file_write_payload;

/// Doc for registry tool `file`; keep in sync with `prompts/file.md`.
const FILE_MD: &str = include_str!("../prompts/file.md");
/// Stable dedup key for system tool appendix (under `crates/pointer-core/src/`).
const FILE_DOC_SOURCE: &str = "tools/prompts/file.md";
/// Standalone schemas for flat file tools (no `method` enum).
const FILE_SCHEMA_YAML: &str = include_str!("../prompts/file.schema.yaml");

pub(crate) const MAX_FILE_READ_BYTES: usize = 256 * 1024;
pub(crate) const MAX_GREP_RESULTS: usize = 200;
pub(crate) const MAX_GREP_FILE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_GLOB_RESULTS: usize = 500;
pub(crate) const MAX_LIST_ENTRIES: usize = 2000;
pub(crate) const MAX_WALK_DEPTH: usize = 64;
pub(crate) const CONTEXT_LINES: usize = 2;


pub fn register_all(reg: &ToolRegistry) {
    let doc = super::tool_doc::doc_markdown_without_schema_fence(FILE_MD);
    let schemas = super::tool_doc::load_tools_from_schema_yaml(FILE_SCHEMA_YAML)
        .expect("file.schema.yaml must be valid");
    let prompt = doc.trim().to_string();

    let explore_guard = |_args: &serde_json::Value, tool_name: &str| -> Result<()> {
        if matches!(
            current_file_tool_lead_profile(),
            Some(AgentProfile::Explore)
        ) {
            warn!("file tool: rejecting mutating tool `{tool_name}` for explore lead profile");
            return Err(anyhow!(
                "Explore worker is read-only: {tool_name} is not allowed."
            ));
        }
        Ok(())
    };

    for (name, schema) in schemas {
        let is_write = name == "file_write" || name == "file_edit";
        let risk = if is_write { "high" } else { "low" };
        let prompt = prompt.clone();

        let handler: ToolHandler = match name.as_str() {
            "file_read" => Arc::new(move |args| {
                let root = resolve_tool_workspace_root()?;
                execute_file_read(&args, &root)
            }),
            "file_write" => {
                Arc::new(move |args| {
                    let root = resolve_tool_workspace_root()?;
                    explore_guard(&args, "file_write")?;
                    execute_file_write_payload(&args, &root)
                })
            }
            "file_edit" => Arc::new(move |args| {
                let root = resolve_tool_workspace_root()?;
                explore_guard(&args, "file_edit")?;
                execute_file_edit_payload(&args, &root)
            }),
            "file_glob" => Arc::new(move |args| {
                let root = resolve_tool_workspace_root()?;
                execute_file_glob_payload(&args, &root)
            }),
            "file_grep" => Arc::new(move |args| {
                let root = resolve_tool_workspace_root()?;
                execute_file_grep_payload(&args, &root)
            }),
            "file_list" => Arc::new(move |args| {
                let root = resolve_tool_workspace_root()?;
                execute_file_list_payload(&args, &root)
            }),
            _ => panic!("Unknown file tool: {name}"),
        };

        reg.register(
            ToolEntry::new(name.clone(), FILE_DOC_SOURCE, risk, is_write, prompt.clone(), handler)
                .with_schema(schema),
        );
    }
}
/// Tool JSON often uses camelCase in schema; models trained on other agents may emit snake_case.
pub(crate) fn json_str<'a>(args: &'a serde_json::Value, camel: &str, snake: &str) -> Option<&'a str> {
    args.get(camel)
        .and_then(|v| v.as_str())
        .or_else(|| args.get(snake).and_then(|v| v.as_str()))
}

pub(crate) fn json_u64_opt(args: &serde_json::Value, camel: &str, snake: &str) -> Option<u64> {
    args.get(camel)
        .and_then(|v| v.as_u64())
        .or_else(|| args.get(snake).and_then(|v| v.as_u64()))
}

#[cfg(test)]
mod tests {
    use super::{
        execute_file_edit_payload, execute_file_glob_payload, execute_file_grep_payload,
        execute_file_list_payload, execute_file_read, execute_file_write_payload,
        resolve_accessible_path, resolve_within_workspace_root, resolve_writable_path,
    };
    use super::edit::try_unique_text_replace;
    use super::path::writable_path_roots;
    use serde_json::json;
    use std::fs;
    use std::io::Write;

    #[test]
    fn file_read_single_path_returns_content_object() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("solo.txt"), "only\n").unwrap();
        let out = execute_file_read(&json!({ "path": "solo.txt" }), root).expect("read");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["content"].as_str().unwrap().contains("only"));
        assert!(v.get("files").is_none());
    }

    #[test]
    fn file_read_accepts_file_alias_and_line_range() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("x.txt"), "p1\np2\np3\np4\n").unwrap();
        let out = execute_file_read(
            &json!({ "file": "x.txt", "lineStart": 2, "lineEnd": 4 }),
            root,
        )
        .expect("read");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["content"].as_str().unwrap(), "p2\np3");
    }

    #[test]
    fn file_read_line_window_ignores_whole_file_max_bytes_gate() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        // Whole file > maxBytes, but the requested line window is tiny.
        let mut body = String::from("head\n");
        body.push_str(&"x".repeat(20_000));
        body.push_str("\ntail-a\ntail-b\n");
        fs::write(root.join("big.txt"), &body).unwrap();
        let out = execute_file_read(
            &json!({
                "path": "big.txt",
                "lineStart": 1,
                "lineEnd": 2,
                "maxBytes": 1000
            }),
            root,
        )
        .expect("line window should not reject on whole-file size");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["content"].as_str().unwrap(), "head");
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn file_read_unbounded_still_rejects_oversized_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("big.txt"), "x".repeat(2000)).unwrap();
        let err = execute_file_read(
            &json!({ "path": "big.txt", "maxBytes": 1000 }),
            root,
        )
        .unwrap_err();
        assert!(err.to_string().contains("文件过大"), "{err}");
    }

    #[test]
    fn file_read_rejects_paths_array() {
        let tmp = tempfile::tempdir().expect("tmp");
        let err = execute_file_read(
            &json!({ "paths": [{ "path": "a.txt" }] }),
            tmp.path(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("path") && msg.contains("paths"), "{msg}");
    }

    #[test]
    fn file_read_missing_path_errors() {
        let tmp = tempfile::tempdir().expect("tmp");
        let err = execute_file_read(&json!({}), tmp.path()).unwrap_err();
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn repro_file_read_windows_unescaped_backslash_args_invalid_json_message() {
        use crate::tools::parse_tool_call_arguments;

        let raw = r#"{"path":"C:\project\pointer-app\src\tools\file\path.rs"}"#;
        assert!(serde_json::from_str::<serde_json::Value>(raw).is_err());
        let args = parse_tool_call_arguments(raw);
        assert!(args.is_null());
        let err = execute_file_read(&args, tempfile::tempdir().unwrap().path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("不是合法 JSON") || msg.contains("正斜杠"), "{msg}");
    }

    #[test]
    fn file_edit_flat_args_ok() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("z.txt"), "foo\n").unwrap();
        let out = execute_file_edit_payload(
            &json!({
                "path": "z.txt",
                "oldString": "foo",
                "newString": "bar"
            }),
            root,
        )
        .expect("edit");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["replaced"], 1);
        assert!(v["path"].as_str().unwrap().contains("z.txt"));
        assert_eq!(fs::read_to_string(root.join("z.txt")).unwrap().trim(), "bar");
    }

    /// Relative write/edit paths must stay under the conversation workspace even when that
    /// workspace is a nested tempfile (broader `temp_dir` is also a writable root).
    #[test]
    fn writable_relative_path_stays_under_workspace_not_home() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let got = resolve_writable_path(root, "z.txt").expect("relative");
        let want = root
            .canonicalize()
            .expect("canon ws")
            .join("z.txt");
        assert_eq!(got, want);
        let home = dirs::home_dir().expect("home");
        assert!(
            !got.starts_with(&home) || want.starts_with(&home),
            "must not redirect relative path to home: got={}",
            got.display()
        );
    }

    #[test]
    fn writable_path_roots_keeps_nested_temp_workspace_first() {
        let tmp = tempfile::tempdir().expect("tmp");
        let ws = tmp.path().canonicalize().expect("canon");
        let roots = writable_path_roots(tmp.path()).expect("roots");
        assert_eq!(roots.first(), Some(&ws));
        let temp_root = std::env::temp_dir()
            .canonicalize()
            .unwrap_or_else(|_| std::env::temp_dir());
        assert!(
            roots.iter().any(|r| r == &temp_root),
            "broader temp_dir should still be listed"
        );
    }

    #[test]
    fn file_edit_accepts_trailing_slash_on_existing_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("z.txt"), "foo\n").unwrap();
        let mut abs = root
            .join("z.txt")
            .canonicalize()
            .expect("canonicalize")
            .to_str()
            .expect("utf8")
            .to_string();
        abs.push('/');
        let out = execute_file_edit_payload(
            &json!({
                "path": abs,
                "oldString": "foo",
                "newString": "bar"
            }),
            root,
        )
        .expect("edit");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(fs::read_to_string(root.join("z.txt")).unwrap().trim(), "bar");
    }

    #[test]
    fn file_edit_rejects_edits_array() {
        let tmp = tempfile::tempdir().expect("tmp");
        let err = execute_file_edit_payload(
            &json!({
                "edits": [
                    { "path": "z.txt", "oldString": "foo", "newString": "bar" }
                ]
            }),
            tmp.path(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("path") && msg.contains("edits"), "{msg}");
    }

    #[test]
    fn file_grep_path_searches_only_that_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("one.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
        fs::write(root.join("two.rs"), "fn alpha_dup() {}\n").unwrap();
        let args = json!({
            "pattern": "alpha",
            "path": "one.rs",
            "maxResults": 20,
        });
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["singleFile"], true);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        let p0 = results[0]["path"].as_str().unwrap();
        assert!(p0.ends_with("one.rs"), "expected absolute path, got {p0}");
    }

    #[test]
    fn file_grep_path_nested_file_single_file_scope() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("pkg").join("a.java"), "class A { void m() {} }\n").unwrap();
        fs::write(root.join("pkg").join("b.java"), "class A { void n() {} }\n").unwrap();
        let args = json!({
            "pattern": "class A",
            "path": "pkg/a.java",
            "maxResults": 20,
        });
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["singleFile"], true);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        let p0 = results[0]["path"].as_str().unwrap();
        assert!(
            p0.ends_with("a.java") && p0.contains("pkg"),
            "expected absolute path under pkg, got {p0}"
        );
    }

    #[test]
    fn file_grep_path_directory_scans_all_files_under() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(root.join("pkg").join("a.java"), "class A {}\n").unwrap();
        fs::write(root.join("pkg").join("b.java"), "class B {}\n").unwrap();
        let args = json!({
            "pattern": "class",
            "path": "pkg",
            "maxResults": 20,
        });
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("singleFile").is_none() || v["singleFile"] == false);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn file_grep_missing_absolute_path_includes_sibling_hints() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("src-tauri")).unwrap();
        fs::write(root.join("src").join("main.rs"), "read_lints\n").unwrap();

        let abs_ui = root.join("ui");
        let args = json!({
            "pattern": "read_lints",
            "path": abs_ui.to_str().unwrap(),
            "maxResults": 20,
        });
        let err = execute_file_grep_payload(&args, root).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("可能的路径"),
            "expected path hints, got: {msg}"
        );
        assert!(msg.contains("src"), "expected src hint, got: {msg}");
    }

    #[test]
    fn file_grep_relative_missing_path_includes_hints() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        let args = json!({"pattern": "foo", "path": "ui", "maxResults": 20});
        let err = execute_file_grep_payload(&args, root).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("可能的路径"), "{msg}");
        assert!(msg.contains("src"), "{msg}");
    }

    #[test]
    fn file_grep_subdir_parameter_rejected() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("x.txt"), "a\n").unwrap();
        let args = json!({
            "pattern": "a",
            "subdir": "x.txt",
        });
        let err = execute_file_grep_payload(&args, root).unwrap_err();
        assert!(err.to_string().contains("subdir"));
        let args_ok = json!({
            "pattern": "a",
            "path": "x.txt",
            "maxResults": 20,
        });
        execute_file_grep_payload(&args_ok, root).expect("grep with path");
    }

    #[test]
    fn file_grep_missing_path_rejected() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("x.txt"), "a\n").unwrap();
        let args = json!({"pattern": "a", "maxResults": 20});
        let err = execute_file_grep_payload(&args, root).unwrap_err();
        assert!(err.to_string().contains("缺少 path"), "{}", err);
    }

    #[test]
    fn file_grep_include_hidden() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        // create hidden file
        fs::create_dir(root.join(".hidden_dir")).unwrap();
        fs::write(root.join(".hidden_dir").join("file.txt"), "secret\n").unwrap();
        // by default, hidden files are skipped
        let args_default = json!({"pattern": "secret", "path": ".", "maxResults": 20});
        let out = execute_file_grep_payload(&args_default, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["count"], 0, "hidden should be skipped by default");
        // includeHidden: true should find
        let args_include = json!({
            "pattern": "secret",
            "path": ".",
            "includeHidden": true,
            "maxResults": 20
        });
        let out2 = execute_file_grep_payload(&args_include, root).expect("grep");
        let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
        assert_eq!(v2["count"], 1, "includeHidden should include hidden");
    }

    #[test]
    fn file_grep_fixed_string() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("f.txt"), "x.y)\n").unwrap();
        // regex would treat '.' as any char and ')' as literal (needs escape). But with fixedString it's literal.
        let args = json!({"pattern": "x.y)", "path": "f.txt", "fixedString": true, "maxResults": 20});
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["count"], 1, "fixedString should match literal");
    }

    #[test]
    fn file_grep_ignore_case() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("case.txt"), "Hello World\n").unwrap();
        // default case sensitive should not match lowercase
        let args_sensitive = json!({"pattern": "hello", "path": "case.txt", "maxResults": 20});
        let out1 = execute_file_grep_payload(&args_sensitive, root).expect("grep");
        let v1: serde_json::Value = serde_json::from_str(&out1).unwrap();
        assert_eq!(v1["count"], 0, "case sensitive should not match lowercase pattern if text is uppercase");
        // ignoreCase: true should match
        let args_ignore = json!({"pattern": "hello", "path": "case.txt", "ignoreCase": true, "maxResults": 20});
        let out2 = execute_file_grep_payload(&args_ignore, root).expect("grep");
        let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
        assert_eq!(v2["count"], 1, "ignoreCase should match");
    }

    #[test]
    fn file_grep_file_types() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("a.rs"), "rust\n").unwrap();
        fs::write(root.join("a.py"), "python\n").unwrap();
        // fileTypes ["rust"] should only scan .rs files and not .py
        let args = json!({"pattern": "rust|python", "path": ".", "fileTypes": ["rust"], "maxResults": 20});
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let results = v["results"].as_array().unwrap();
        // should only have one result from a.rs
        assert_eq!(results.len(), 1, "fileTypes should restrict to .rs");
        let path = results[0]["path"].as_str().unwrap();
        assert!(path.ends_with("a.rs"), "expected a.rs, got {}", path);
    }

    #[test]
    fn file_grep_file_types_rs_alias() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("a.rs"), "rust\n").unwrap();
        fs::write(root.join("a.py"), "python\n").unwrap();
        let args = json!({"pattern": "rust|python", "path": ".", "fileTypes": ["rs"], "maxResults": 20});
        let out = execute_file_grep_payload(&args, root).expect("grep");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 1, "fileTypes rs alias should restrict to .rs");
        assert!(results[0]["path"].as_str().unwrap().ends_with("a.rs"));
    }

    #[test]
    fn file_grep_file_types_yaml_and_cpp_aliases() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("cfg.yml"), "key: val\n").unwrap();
        fs::write(root.join("main.cpp"), "int main() {}\n").unwrap();
        fs::write(root.join("readme.md"), "key: val\n").unwrap();

        let args_yaml = json!({"pattern": "key:", "path": ".", "fileTypes": ["yml"], "maxResults": 20});
        let out_yaml = execute_file_grep_payload(&args_yaml, root).expect("grep yaml");
        let v_yaml: serde_json::Value = serde_json::from_str(&out_yaml).unwrap();
        assert_eq!(v_yaml["count"], 1);
        assert!(v_yaml["results"][0]["path"].as_str().unwrap().ends_with("cfg.yml"));

        let args_cpp = json!({"pattern": "main", "path": ".", "fileTypes": ["c++"], "maxResults": 20});
        let out_cpp = execute_file_grep_payload(&args_cpp, root).expect("grep cpp");
        let v_cpp: serde_json::Value = serde_json::from_str(&out_cpp).unwrap();
        assert_eq!(v_cpp["count"], 1);
        assert!(v_cpp["results"][0]["path"].as_str().unwrap().ends_with("main.cpp"));
    }

    #[test]
    fn file_grep_include_exclude_globs() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("test")).unwrap();
        fs::write(root.join("src").join("lib.rs"), "code\n").unwrap();
        fs::write(root.join("test").join("test.rs"), "code\n").unwrap();
        // include glob: src/**/*
        let args_inc = json!({
            "pattern": "code",
            "path": ".",
            "includeGlobs": ["src/**/*"],
            "maxResults": 20
        });
        let out1 = execute_file_grep_payload(&args_inc, root).expect("grep");
        let v1: serde_json::Value = serde_json::from_str(&out1).unwrap();
        assert_eq!(v1["count"], 1, "includeGlobs should filter");
        // exclude glob: test/**/*
        let args_exc = json!({
            "pattern": "code",
            "path": ".",
            "excludeGlobs": ["test/**/*"],
            "maxResults": 20
        });
        let out2 = execute_file_grep_payload(&args_exc, root).expect("grep");
        let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
        assert_eq!(v2["count"], 1, "excludeGlobs should filter");
    }

    #[test]
    fn resolve_rejects_parent_escape() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let err = resolve_within_workspace_root(root, "../outside").unwrap_err();
        assert!(err.to_string().contains("工作区") || err.to_string().contains("越出"));
    }

    #[test]
    fn resolve_accepts_absolute_path_under_workspace_for_new_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let abs = root.join("nested").join("new.txt");
        let got = resolve_within_workspace_root(root, abs.to_str().unwrap()).unwrap();
        let root_c = root.canonicalize().unwrap();
        assert!(got.starts_with(&root_c));
        assert!(got.ends_with("new.txt"));
    }

    #[test]
    fn resolve_accepts_absolute_path_under_workspace_existing_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let p = root.join("z.txt");
        fs::write(&p, "z").unwrap();
        let abs = p.canonicalize().unwrap();
        let got = resolve_within_workspace_root(root, abs.to_str().unwrap()).unwrap();
        assert_eq!(got, abs);
    }

    #[test]
    fn resolve_rejects_absolute_path_outside_workspace() {
        let ws = tempfile::tempdir().expect("tmp");
        let other = tempfile::tempdir().expect("tmp");
        let f = other.path().join("x.txt");
        fs::write(&f, "x").unwrap();
        let abs = f.canonicalize().unwrap();
        let err = resolve_within_workspace_root(ws.path(), abs.to_str().unwrap()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("工作区") || msg.contains("不在"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn writable_path_accepts_absolute_under_temp() {
        let ws = tempfile::tempdir().expect("tmp");
        let temp = std::env::temp_dir();
        let dir = temp.join(format!(
            "pointer-writable-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("out.txt");
        fs::write(&target, "").unwrap();
        let got = resolve_writable_path(ws.path(), target.to_str().unwrap()).expect("temp write");
        assert_eq!(
            got.canonicalize().unwrap(),
            target.canonicalize().unwrap()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writable_path_accepts_tilde_home_file() {
        let ws = tempfile::tempdir().expect("tmp");
        let home = dirs::home_dir().expect("home");
        let dir = home.join(format!(".pointer-writable-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("note.txt");
        let tilde = format!("~/.pointer-writable-test-{}/note.txt", std::process::id());
        let got = resolve_writable_path(ws.path(), &tilde).expect("home tilde write");
        assert_eq!(got, target);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writable_path_rejects_system_path_outside_roots() {
        let ws = tempfile::tempdir().expect("tmp");
        #[cfg(unix)]
        let outside = "/etc/hosts";
        #[cfg(windows)]
        let outside = "C:\\Windows\\System32\\drivers\\etc\\hosts";
        let err = resolve_writable_path(ws.path(), outside).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("写入") || msg.contains("允许"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn accessible_path_expands_tilde_home() {
        let ws = tempfile::tempdir().expect("tmp");
        let home = dirs::home_dir().expect("home");
        let got = resolve_accessible_path(ws.path(), "~").expect("tilde");
        let want = home.canonicalize().unwrap_or(home);
        assert_eq!(got, want);
    }

    #[test]
    fn accessible_path_absolute_outside_workspace() {
        let ws = tempfile::tempdir().expect("tmp");
        let other = tempfile::tempdir().expect("tmp");
        let f = other.path().join("out.txt");
        fs::write(&f, "x").unwrap();
        let abs = f.canonicalize().unwrap();
        let got = resolve_accessible_path(ws.path(), abs.to_str().unwrap()).unwrap();
        assert_eq!(got, abs);
    }

    #[test]
    fn file_list_non_recursive() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("a.txt"), "1").unwrap();
        fs::write(root.join("b.txt"), "2").unwrap();
        let args = json!({"path": ".", "recursive": false, "entryType": "all"});
        let out = execute_file_list_payload(&args, root).expect("list");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let paths: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert!(paths.iter().any(|p| p.ends_with("b.txt")));
        assert!(paths.iter().any(|p| p.ends_with("sub") && !p.ends_with("sub\\a.txt")));
    }

    #[test]
    fn resolve_allows_nested_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let sub = root.join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        let f = sub.join("x.txt");
        let mut file = fs::File::create(&f).unwrap();
        writeln!(file, "hi").unwrap();

        let got = resolve_within_workspace_root(root, "a/b/x.txt").unwrap();
        assert_eq!(got, f.canonicalize().unwrap());
    }

    #[test]
    fn text_replace_lf_snippet_matches_crlf_file() {
        let text = "line1\r\nline2\r\n";
        let out = try_unique_text_replace(text, "line1\nline2", "A\nB").unwrap();
        assert_eq!(out, "A\r\nB\r\n");
    }

    #[test]
    fn text_replace_exact_lf_file() {
        let text = "line1\nline2\n";
        let out = try_unique_text_replace(text, "line1\nline2", "X\nY").unwrap();
        assert_eq!(out, "X\nY\n");
    }

    #[test]
    fn text_replace_crlf_snippet_matches_lf_file() {
        let text = "line1\nline2\n";
        let out = try_unique_text_replace(text, "line1\r\nline2", "P\r\nQ").unwrap();
        assert_eq!(out, "P\nQ\n");
    }

    #[test]
    fn text_replace_lf_snippet_matches_cr_only_file() {
        let text = "line1\rline2\r";
        let out = try_unique_text_replace(text, "line1\nline2", "A\nB").unwrap();
        assert_eq!(out, "A\nB\n");
    }

    #[test]
    fn file_glob_default_entry_type_matches_only_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("only.txt"), "x").unwrap();
        fs::create_dir_all(root.join("empty_dir")).unwrap();
        let args = json!({
            "pattern": "**/*",
            "maxResults": 50
        });
        let out = execute_file_glob_payload(&args, root).expect("glob");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["entryType"], "file");
        let matches: Vec<&str> = v["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].ends_with("only.txt"));
    }

    #[test]
    fn file_glob_entry_type_dir_finds_dot_git_with_include_hidden() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let args = json!({
            "pattern": "**/.git",
            "entryType": "dir",
            "includeHidden": true,
            "maxResults": 20
        });
        let out = execute_file_glob_payload(&args, root).expect("glob");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["entryType"], "dir");
        assert_eq!(v["includeHidden"], true);
        let matches: Vec<&str> = v["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        assert!(
            matches.iter().any(|p| p.replace('\\', "/").ends_with("/.git")),
            "expected a .git directory in matches: {matches:?}"
        );
    }

    #[test]
    fn file_glob_skips_under_hidden_dir_without_include_hidden() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let nested = root.join(".hidden").join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("leaf.txt"), "y").unwrap();
        let args = json!({
            "pattern": "**/leaf.txt",
            "maxResults": 20
        });
        let out = execute_file_glob_payload(&args, root).expect("glob");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["count"], 0);

        let args_inc = json!({
            "pattern": "**/leaf.txt",
            "includeHidden": true,
            "maxResults": 20
        });
        let out2 = execute_file_glob_payload(&args_inc, root).expect("glob");
        let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
        assert_eq!(v2["count"], 1);
    }

    #[test]
    fn file_glob_dot_pointer_from_parent_needs_include_hidden() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let skill = root.join(".pointer").join("skills").join("demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "body").unwrap();
        let args = json!({
            "pattern": ".pointer/skills/demo/**/*.md",
            "maxResults": 20
        });
        let out = execute_file_glob_payload(&args, root).expect("glob");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["count"], 0);

        let args_inc = json!({
            "pattern": ".pointer/skills/demo/**/*.md",
            "includeHidden": true,
            "maxResults": 20
        });
        let out2 = execute_file_glob_payload(&args_inc, root).expect("glob");
        let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
        assert_eq!(v2["count"], 1, "matches={}", v2["matches"]);
    }

    #[test]
    fn file_glob_absolute_pattern_ignores_wrong_base_and_finds_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let skill = root.join(".pointer").join("skills").join("cwpt-reimburse-submit");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "body").unwrap();
        let abs_pattern = format!("{}/**/*.md", skill.display()).replace('\\', "/");
        let args = json!({
            "pattern": abs_pattern,
            "base": root.display().to_string(),
            "maxResults": 20
        });
        let out = execute_file_glob_payload(&args, root).expect("glob");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["count"], 1, "matches={}", v["matches"]);
        assert_eq!(v["pattern"], "**/*.md");
    }

    #[test]
    fn file_glob_rejects_invalid_entry_type() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        fs::write(root.join("z.txt"), "z").unwrap();
        let args = json!({
            "pattern": "*.txt",
            "entryType": "bogus",
            "maxResults": 10
        });
        let err = execute_file_glob_payload(&args, root).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("entryType") || msg.contains("无效"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn file_write_accepts_string_content() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let args = json!({
            "path": "out.txt",
            "content": "hello\n"
        });
        execute_file_write_payload(&args, root).expect("write");
        assert_eq!(fs::read_to_string(root.join("out.txt")).unwrap(), "hello\n");
    }

    #[test]
    fn file_write_accepts_object_content_as_pretty_json() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let args = json!({
            "path": "package.json",
            "content": {
                "name": "llm-chat",
                "private": true,
                "version": "0.1.0"
            }
        });
        execute_file_write_payload(&args, root).expect("write");
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("package.json")).unwrap()).unwrap();
        assert_eq!(written["name"], "llm-chat");
        assert_eq!(written["private"], true);
    }
}
