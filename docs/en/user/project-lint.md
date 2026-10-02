# `read_lints` hybrid configuration

English | [简体中文](../../zh-CN/user/project-lint.md)

> User guide index: [`../user/README.md`](../user/README.md). What follows is the complete configuration reference for the workspace **`.pointer/lint.toml`**.

The `read_lints` tool (Coder agent only; Rust sources under `crates/pointer-core/src/agents/coder/read_lints`) combines **auto-detected** linters for common language ecosystems (JavaScript/TypeScript, Python, Rust, Go, Java, C#, PHP, Ruby, Swift, Dart — see the embedded tool prompt) with optional **workspace-local** commands in **`.pointer/lint.toml`**.

**C / C++** and other stacks are **not** auto-detected: add **`[[commands]]`** (for example `cmake --build …`, `clang-tidy …`) or use the **`terminal`** tool.

## File location

- Path: **`<workspace_root>/.pointer/lint.toml`**
- The workspace root is the same directory the app uses for `file` / `terminal` tools (`workspaceRoot` in settings).

## Schema

```toml
[[commands]]
shell = "one shell line run from workspace root"
parser = "text-on-failure"
```

Repeat `[[commands]]` for multiple steps. Commands run **after** built-in stacks when `stack` is `auto`.

### `shell`

A single line executed like a typed shell command:

- **Windows**: `cmd.exe /C <shell>`
- **macOS / Linux**: `sh -lc <shell>`

Use the same portability habits as in CI (quote paths, avoid interactive prompts).

### `parser`

| Value | When to use |
| --- | --- |
| `text-on-failure` | Default. If exit code ≠ 0, emit one diagnostic with a truncated stdout/stderr tail. Exit 0 → no diagnostics from this command. |
| `eslint-json` | stdout is ESLint’s **`-f json`** array. |
| `ruff-json` | stdout is **Ruff** `check --output-format=json` array. |
| `cargo-json-lines` | stdout is **Cargo / rustc** `--message-format=json` (one JSON object per line); only `compiler-message` reasons are kept. |
| `maven-log` | stdout **and** stderr are scanned for Maven `[ERROR] File.java:[line,col]` and javac-style `File.java:line: error:` lines (same heuristics as the built-in Java stack). |

Unknown `parser` values are rejected with an error for that entry.

### Built-in Java (no config file)

When **`pom.xml`** exists at the workspace root, the tool runs **`mvn --batch-mode -q -DskipTests compile`** (or **`mvnw.cmd` / `./mvnw`** if the wrapper is present). When there is **no** `pom.xml` but **`build.gradle`** or **`build.gradle.kts`** exists, it runs **`gradlew.bat` / `./gradlew` compileJava`** (or **`gradle`** if there is no wrapper). Parsed diagnostics use the same `maven-log` patterns as above. **Checkstyle / SpotBugs / PMD** are not invoked automatically—use **`[[commands]]`** below or the **`terminal`** tool.

## Examples

### Maven compile log (custom command)

If you run Maven yourself and want structured `.java` issues:

```toml
[[commands]]
shell = "mvn -q -DskipTests compile"
parser = "maven-log"
```

### Maven Checkstyle

```toml
[[commands]]
shell = "mvn -q checkstyle:check"
parser = "text-on-failure"
```

### npm script that runs ESLint with JSON to stdout

Ensure the script prints ESLint JSON to stdout (not only human text). Then:

```toml
[[commands]]
shell = "npm run lint -- --format json"
parser = "eslint-json"
```

(Exact flags depend on your `package.json` scripts; align with what CI runs.)

## `stack` tool argument

- **`auto`** — built-ins + `.pointer/lint.toml` (if present).
- **`config`** — **only** `.pointer/lint.toml` (no built-in detection). Use for repositories where none of the built-in detectors apply.
- **`rust`** / **`node`** / **`python`** / **`go`** / **`java`** / **`maven`** / **`gradle`** — force a single built-in stack when detectors allow it (`java` picks Maven if `pom.xml` exists, else Gradle when a Gradle build file exists).

## Monorepos (JavaScript / TypeScript / Vue)

Built-in **ESLint** and **Oxlint** detection starts at the **workspace root**. If the root has no JS linter signal but you pass **`paths`** under a sub-package that does, `read_lints` walks up from those paths and runs the linter with that package as **`cwd`**. Lockfiles are resolved from the package directory up toward the workspace root.

Vue and TypeScript are covered when your package’s ESLint or Oxlint setup lints those files (there is no separate built-in `vue-tsc` or `tsc` unless you add **`[[commands]]`**).

Tool responses include **`lintExecuted`**, **`outcome`**, and **`summary`**. Empty **`diagnostics`** with **`lintExecuted: false`** means no check ran—not a clean result.

## Relation to CI

Prefer the **same commands** as CI so agent results match pipeline expectations. The tool does not parse `pom.xml` or `package.json` to discover goals automatically; use `.pointer/lint.toml` to mirror `mvn …` or `gradle …` explicitly.
