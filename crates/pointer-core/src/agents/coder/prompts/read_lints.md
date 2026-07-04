---
schema:
  type: object
  properties:
    stack:
      type: string
    paths:
      type: array
      items:
        type: string
    timeoutMs:
      type: integer
      minimum: 10000
  additionalProperties: true
---

### `read_lints`

Run **structured static checks** for the configured workspace and return a unified `diagnostics[]`
(`severity`, `message`, `path`, `line`, `column`, `code`, `source`).
This does **not** replace unit tests.

**Built-in coverage (common “top languages” ecosystems)**

| Area | Detection (workspace root) | Command / output |
|------|----------------------------|------------------|
| **JavaScript / TypeScript** | `package.json` + ESLint config or `eslint` dependency | `npx` / `pnpm` / `yarn` + `eslint -f json` |
| **JS/TS (Oxlint)** | `package.json` + `.oxlintrc.json` or `oxlint` dependency | `npx` / `pnpm` / `yarn` + `oxlint -f json` |
| **Python** | `ruff.toml` or `[tool.ruff]` in `pyproject.toml` | `ruff check --output-format=json` |
| **Rust** | `Cargo.toml` | `cargo clippy --message-format=json` |
| **Go** | `go.mod` | `golangci-lint run --out-format=json` |
| **Java** | `pom.xml` → Maven; else `build.gradle` / `build.gradle.kts` → Gradle | `mvn compile` / `gradlew compileJava` + log parse |
| **C# / .NET** | `*.sln` or `*.csproj` in root | `dotnet build` + MSBuild line parse |
| **PHP** | `composer.json` + PHPStan config/deps | `composer exec phpstan analyse --error-format=json` |
| **Ruby** | `Gemfile` + RuboCop config or gem | `bundle exec rubocop --format=json` (or `rubocop`) |
| **Swift** | `Package.swift` | `swift build` + compiler line parse |
| **Dart / Flutter** | `pubspec.yaml` | `dart analyze --format=json` |

**Not built-in (use `.pointer/lint.toml` or `terminal`)**

- **C / C++** (clang-tidy, cppcheck, CMake presets), **Kotlin-only** without Gradle, **SQL**, **Shell**, and other stacks: add **`[[commands]]`** or run your CI script via **`terminal`**.

**Hybrid design**

1. **Built-in stacks** above run when `stack` is **`auto`** or omitted, in a fixed order (see table).
2. **Optional `.pointer/lint.toml`**: extra shell commands; appended **after** built-ins when `stack` is `auto`.
3. Anything else: **`terminal`**.

**`stack` parameter**

- **`auto`** (default) — built-ins that match the repo + `.pointer/lint.toml` if present.
- **`config`** — **only** `.pointer/lint.toml`.
- **`java`** — Maven or Gradle only (see Java row).
- **`rust`** | **`node`** | **`oxlint`** | **`python`** | **`go`** | **`maven`** / **`java-maven`** | **`gradle`** / **`java-gradle`** | **`dotnet`** | **`php`** / **`phpstan`** | **`ruby`** / **`rubocop`** | **`swift`** | **`dart`** / **`flutter`** — one built-in stack if detectors allow it; otherwise empty plan + `hint`.

**Other parameters**

- **`paths`** — optional path strings; filters merged diagnostics (and ESLint/Ruff targets when supported).
- **`timeoutMs`** — total wall budget, **split evenly** across planned runs (minimum **10000** ms per run, capped by the total).

**Response shape**

- **`lintExecuted`** — `true` only when at least one subprocess actually ran (not skipped).
- **`outcome`** — `skipped_no_matching_stack` | `skipped_all_runs_failed` | `clean` | `issues_found` | `tool_failed`.
- **`summary`** — short human-readable status; **use this** when reporting to the user.
- **`runs`** — per subprocess: `engine`, `exitCode`, `timedOut`, `stderrTailUtf8`, `skipped` / `reason`, optional `cwd` (monorepo package).
- **`diagnostics`** — merged list (cap **200**; `diagnosticsTruncated` when exceeded).
- **`perRunWallMs`** — timeout slice per run.

**Reporting rules**

- **`runs: []`** or **`lintExecuted: false`** → static check **did not run**; do **not** claim “no lint errors”.
- **`outcome: clean`** → check ran and reported no issues for the requested scope.
- **`outcome: issues_found`** → check ran; fix or explain listed diagnostics.
- **`outcome: tool_failed`** → a linter subprocess ran but exited with errors (e.g. missing toolchain, command not found); read **`runs.stderrTailUtf8`** — do **not** treat as clean.
- Empty **`diagnostics`** with **`lintExecuted: false`** is **not** a clean bill of health.
- Empty **`diagnostics`** with **`outcome: tool_failed`** is **not** a clean bill of health.

**When `outcome: tool_failed`**

1. Read **`runs.stderrTailUtf8`** and identify whether the failure is a **missing component or command** (stderr often includes an install hint, e.g. `rustup component add clippy`).
2. If stderr gives a **concrete install/fix command** and the user has **not** forbidden environment changes in this thread, run that command via **`terminal`**, then **call `read_lints` again** with the same **`stack`** / **`paths`**.
3. Tell the user what you ran and the retry outcome. If install fails or stderr has no safe one-liner, stop and ask the user — do **not** claim lint-clean.
4. If you only changed one stack (e.g. frontend) and another stack’s tool is missing, you may **first** retry with a narrower **`stack`** (e.g. `eslint`) and **`paths`**; otherwise follow steps 1–3 for the failed engine.
5. **Do not** treat **`tool_failed`** as clean. Avoid **`brew install`**, **`npm install -g`**, or other broad system changes unless stderr explicitly requires them and the user has not objected.

**Monorepos (JS/TS/Vue)**

Built-in **ESLint** or **Oxlint** is detected at the **workspace root** first.
When root has neither but **`paths`** point under a sub-package (`package.json` + ESLint or Oxlint config), that package root is used automatically.
Lockfiles (`pnpm-lock.yaml`, `yarn.lock`) are resolved from the package directory upward.
Vue/TypeScript are covered via **ESLint** or **Oxlint** (not standalone `vue-tsc` / `tsc` unless you add `.pointer/lint.toml`).

**Cross-platform**

Config entries use the same shell rules as **`terminal`**: Windows `cmd /C`, macOS/Linux `sh -lc`.

#### `.pointer/lint.toml`

See **`docs/user/project-lint.md`**. **`parser`** values include `eslint-json`, `ruff-json`, `cargo-json-lines`, **`maven-log`**, `text-on-failure`.
