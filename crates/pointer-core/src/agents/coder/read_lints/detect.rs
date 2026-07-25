//! Detect which built-in lint stacks apply to a workspace root.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinStack {
    NodeEslint,
    NodeOxlint,
    PythonRuff,
    RustClippy,
    GoGolangci,
    JavaMaven,
    JavaGradle,
    /// C# / .NET — `dotnet build` on a root `.sln` or `.csproj`.
    Dotnet,
    /// PHP — `composer exec phpstan` when PHPStan is configured.
    PhpStan,
    /// Ruby — `bundle exec rubocop --format=json` when RuboCop is configured.
    Rubocop,
    /// Swift — `swift build` log parsing.
    SwiftPm,
    /// Dart / Flutter — `dart analyze --format=json`.
    DartAnalyze,
}

pub fn config_file_path(root: &Path) -> PathBuf {
    root.join(".pointer").join("lint.toml")
}

fn package_json_path(root: &Path) -> PathBuf {
    root.join("package.json")
}

fn read_package_json(root: &Path) -> Option<serde_json::Value> {
    let p = package_json_path(root);
    let s = fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}

fn has_eslint_signal(root: &Path) -> bool {
    if root.join("eslint.config.js").is_file()
        || root.join("eslint.config.mjs").is_file()
        || root.join("eslint.config.cjs").is_file()
        || root.join("eslint.config.ts").is_file()
        || root.join(".eslintrc.js").is_file()
        || root.join(".eslintrc.cjs").is_file()
        || root.join(".eslintrc.json").is_file()
        || root.join(".eslintrc.yaml").is_file()
        || root.join(".eslintrc.yml").is_file()
    {
        return true;
    }
    let Some(pkg) = read_package_json(root) else {
        return false;
    };
    for key in ["devDependencies", "dependencies", "peerDependencies"] {
        if let Some(o) = pkg.get(key).and_then(|x| x.as_object()) {
            if o.contains_key("eslint") {
                return true;
            }
        }
    }
    false
}

pub fn detect_node_eslint(root: &Path) -> bool {
    package_json_path(root).is_file() && has_eslint_signal(root)
}

fn has_oxlint_signal(root: &Path) -> bool {
    if root.join(".oxlintrc.json").is_file()
        || root.join(".oxlintrc.jsonc").is_file()
        || root.join("oxlint.config.json").is_file()
        || root.join("oxlint.config.jsonc").is_file()
    {
        return true;
    }
    let Some(pkg) = read_package_json(root) else {
        return false;
    };
    for key in ["devDependencies", "dependencies", "peerDependencies"] {
        if let Some(o) = pkg.get(key).and_then(|x| x.as_object()) {
            if o.contains_key("oxlint") {
                return true;
            }
        }
    }
    false
}

pub fn detect_node_oxlint(root: &Path) -> bool {
    package_json_path(root).is_file() && has_oxlint_signal(root)
}

/// Nearest directory from `cwd` up to `workspace` with `pnpm-lock.yaml` or `yarn.lock`.
pub fn find_node_lock_root(cwd: &Path, workspace: &Path) -> PathBuf {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let mut cur = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    loop {
        if cur.join("pnpm-lock.yaml").is_file() || cur.join("yarn.lock").is_file() {
            return cur;
        }
        if cur == workspace {
            return workspace;
        }
        if !cur.pop() {
            return workspace;
        }
    }
}

fn find_node_tool_project_root(
    start: &Path,
    workspace: &Path,
    detect: fn(&Path) -> bool,
) -> Option<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let mut cur = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let cur_canon = cur.canonicalize().unwrap_or_else(|_| cur.clone());
        if !cur_canon.starts_with(&workspace) {
            return None;
        }
        if detect(&cur_canon) {
            return Some(cur_canon);
        }
        if cur_canon == workspace {
            return None;
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn node_tool_project_roots_from_filters(
    workspace: &Path,
    filters: &[PathBuf],
    detect: fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for f in filters {
        let Some(pkg) = find_node_tool_project_root(f, workspace, detect) else {
            continue;
        };
        if !out.iter().any(|p| p == &pkg) {
            out.push(pkg);
        }
    }
    out
}

/// Monorepos: when workspace root has no ESLint, infer package roots from `paths` filters.
pub fn eslint_project_roots_from_filters(workspace: &Path, filters: &[PathBuf]) -> Vec<PathBuf> {
    node_tool_project_roots_from_filters(workspace, filters, detect_node_eslint)
}

/// Monorepos: infer Oxlint package roots from `paths` filters.
pub fn oxlint_project_roots_from_filters(workspace: &Path, filters: &[PathBuf]) -> Vec<PathBuf> {
    node_tool_project_roots_from_filters(workspace, filters, detect_node_oxlint)
}

pub fn pyproject_has_ruff_section(root: &Path) -> bool {
    let p = root.join("pyproject.toml");
    let Ok(s) = fs::read_to_string(p) else {
        return false;
    };
    s.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("[tool.ruff]") || t.starts_with("[tool.ruff.")
    })
}

pub fn detect_python_ruff(root: &Path) -> bool {
    root.join("ruff.toml").is_file() || pyproject_has_ruff_section(root)
}

pub fn detect_rust_clippy(root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
}

pub fn detect_go(root: &Path) -> bool {
    root.join("go.mod").is_file()
}

/// Prefer Maven when `pom.xml` exists; otherwise Gradle when build scripts exist.
pub fn pick_java_stack(root: &Path) -> Option<BuiltinStack> {
    if root.join("pom.xml").is_file() {
        return Some(BuiltinStack::JavaMaven);
    }
    if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        return Some(BuiltinStack::JavaGradle);
    }
    None
}

pub fn detect_java_maven(root: &Path) -> bool {
    root.join("pom.xml").is_file()
}

pub fn detect_java_gradle(root: &Path) -> bool {
    !root.join("pom.xml").is_file()
        && (root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file())
}

fn root_has_dotnet_marker(root: &Path) -> bool {
    let Ok(rd) = root.read_dir() else {
        return false;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let s = name.to_string_lossy();
        if s.ends_with(".sln") || s.ends_with(".csproj") {
            return true;
        }
    }
    false
}

pub fn detect_dotnet(root: &Path) -> bool {
    root_has_dotnet_marker(root)
}

pub fn detect_phpstan(root: &Path) -> bool {
    if !root.join("composer.json").is_file() {
        return false;
    }
    if root.join("phpstan.neon").is_file() || root.join("phpstan.dist.neon").is_file() {
        return true;
    }
    let Ok(s) = fs::read_to_string(root.join("composer.json")) else {
        return false;
    };
    s.contains("phpstan/phpstan") || s.contains("\"phpstan\"") || s.contains("'phpstan'")
}

pub fn detect_rubocop(root: &Path) -> bool {
    if !root.join("Gemfile").is_file() {
        return false;
    }
    if root.join(".rubocop.yml").is_file() || root.join(".rubocop_todo.yml").is_file() {
        return true;
    }
    fs::read_to_string(root.join("Gemfile"))
        .ok()
        .is_some_and(|s| s.contains("rubocop"))
}

pub fn detect_swift_pm(root: &Path) -> bool {
    root.join("Package.swift").is_file()
}

pub fn detect_dart_pub(root: &Path) -> bool {
    root.join("pubspec.yaml").is_file()
}

/// Default discovery order: lighter / faster tools before heavier compiles.
pub fn detect_builtin_stacks(root: &Path) -> Vec<BuiltinStack> {
    let mut v = Vec::new();
    if detect_node_eslint(root) {
        v.push(BuiltinStack::NodeEslint);
    }
    if detect_node_oxlint(root) {
        v.push(BuiltinStack::NodeOxlint);
    }
    if detect_python_ruff(root) {
        v.push(BuiltinStack::PythonRuff);
    }
    if detect_rust_clippy(root) {
        v.push(BuiltinStack::RustClippy);
    }
    if detect_go(root) {
        v.push(BuiltinStack::GoGolangci);
    }
    if let Some(j) = pick_java_stack(root) {
        v.push(j);
    }
    if detect_dotnet(root) {
        v.push(BuiltinStack::Dotnet);
    }
    if detect_phpstan(root) {
        v.push(BuiltinStack::PhpStan);
    }
    if detect_rubocop(root) {
        v.push(BuiltinStack::Rubocop);
    }
    if detect_swift_pm(root) {
        v.push(BuiltinStack::SwiftPm);
    }
    if detect_dart_pub(root) {
        v.push(BuiltinStack::DartAnalyze);
    }
    v
}

/// Map a file extension to the built-in stacks that can lint it.
///
/// Covers the top 20+ programming languages.  Extensions that map to an
/// empty slice represent languages without a built-in stack — when all
/// paths consist of such extensions the fallback in [`filter_stacks_by_paths`]
/// returns the full detected stack list.
fn stacks_for_extension(ext: &str) -> &'static [BuiltinStack] {
    // ── Languages with built-in lint stacks ──────────────────────────
    match ext {
        // Rust
        "rs" => &[BuiltinStack::RustClippy],

        // JavaScript / TypeScript / JSX / TSX / Vue / Svelte / Astro
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "vue" | "svelte" | "astro" => {
            &[BuiltinStack::NodeEslint, BuiltinStack::NodeOxlint]
        }

        // Python
        "py" | "pyi" => &[BuiltinStack::PythonRuff],

        // Go
        "go" => &[BuiltinStack::GoGolangci],

        // Java / Kotlin / Groovy (JVM)
        "java" => &[BuiltinStack::JavaMaven, BuiltinStack::JavaGradle],
        "kt" | "kts" | "groovy" => &[BuiltinStack::JavaGradle],

        // C# / .NET
        "cs" => &[BuiltinStack::Dotnet],

        // PHP
        "php" => &[BuiltinStack::PhpStan],

        // Ruby
        "rb" => &[BuiltinStack::Rubocop],

        // Swift
        "swift" => &[BuiltinStack::SwiftPm],

        // Dart
        "dart" => &[BuiltinStack::DartAnalyze],

        // ── Languages without a built-in stack ───────────────────────
        // (empty slice → filtered out; filter_stacks_by_paths fallback
        //  returns all stacks when everything is filtered)

        // C / C++
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hxx" => &[],

        // Objective-C
        "m" | "mm" => &[],

        // Scala
        "scala" | "sc" => &[],

        // R
        "r" | "R" => &[],

        // Shell
        "sh" | "bash" | "zsh" => &[],

        // Lua
        "lua" => &[],

        // Perl
        "pl" | "pm" => &[],

        // Zig
        "zig" => &[],

        // Elixir
        "ex" | "exs" => &[],

        // Haskell
        "hs" => &[],

        // Clojure
        "clj" | "cljs" | "cljc" => &[],

        // Erlang
        "erl" => &[],

        // Julia
        "jl" => &[],

        // Nim
        "nim" => &[],

        // Terraform
        "tf" | "tfvars" => &[],

        // Solidity
        "sol" => &[],

        // SQL
        "sql" => &[],

        // Markdown / MDX
        "md" | "mdx" => &[],

        // YAML
        "yaml" | "yml" => &[],

        // TOML
        "toml" => &[],

        // JSON
        "json" => &[],

        // CSS / SCSS / SASS / Less
        "css" | "scss" | "sass" | "less" => &[],

        // HTML
        "html" | "htm" => &[],

        // GraphQL
        "graphql" | "gql" => &[],

        // Protobuf
        "proto" => &[],

        // Everything else (unknown extension)
        _ => &[],
    }
}

/// When `paths` is non-empty, keep only built-in stacks whose language
/// matches at least one path extension.  Falls back to the full list when
/// any path has no recognisable extension (e.g. a directory) or when
/// filtering would remove everything.
pub fn filter_stacks_by_paths(stacks: Vec<BuiltinStack>, paths: &[PathBuf]) -> Vec<BuiltinStack> {
    if paths.is_empty() {
        return stacks;
    }

    // Collect the union of all stacks that any path extension maps to.
    let mut relevant: Vec<BuiltinStack> = Vec::new();
    let mut has_non_extension_path = false;

    for p in paths {
        match p.extension().and_then(|e| e.to_str()) {
            Some(ext) => {
                for s in stacks_for_extension(ext) {
                    if !relevant.contains(s) {
                        relevant.push(*s);
                    }
                }
            }
            None => {
                // Directory or extensionless path — play it safe.
                has_non_extension_path = true;
            }
        }
    }

    if has_non_extension_path {
        return stacks;
    }

    let filtered: Vec<BuiltinStack> = stacks
        .iter()
        .filter(|s| relevant.contains(s))
        .copied()
        .collect();

    // If the filter removed everything but we started with something,
    // keep the original list — better to run extra linters than none.
    if filtered.is_empty() {
        stacks
    } else {
        filtered
    }
}
