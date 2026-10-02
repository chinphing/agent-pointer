# Coder offline evaluation setup: a local guide

English | [简体中文](../../zh-CN/contributing/coder-agent-offline-eval-setup.md)

> **Audience**: you want to run "repeatably scored" regression tasks on your own machine, to evaluate prompt / model / tool changes.  
> **Note**: "offline" is understood below at **three layers**; you can implement only one of them as needed.

---

## 1. What "offline" means (align expectations first)

| Layer | Meaning | Typical implementation |
| --- | --- | --- |
| **A. Scoring offline** | No manual UI clicking; pass/fail is decided by a script | Shell / `cargo test` / file grep |
| **B. Runner offline** | The task environment and dependencies are reproducible and do not rely on your online services | A pinned Git tag, Docker, or locked `cargo` versions |
| **C. LLM offline** | Model inference does not reach the public API | Local **Ollama** / **vLLM** etc., OpenAI-compatible `http://127.0.0.1:…/v1` |

By default Pointer goes through **cloud services such as DashScope**; to do **C**, just change the Base URL and model name in the app's "Settings" to a local compatible endpoint (it may still reach the LAN beyond the local machine, depending on the deployment).

---

## 2. Minimum viable setup (recommended first step): "golden task + scripted scoring"

Even without wiring up the full Agent, you can first build half of **A + B**: verify the "repository state → command → exit code/output" chain.

### 2.1 Suggested layout (beside the main repository or in a subdirectory)

```text
eval/
  fixtures/
    tiny-rust-bug/          # small Rust project, or a git submodule pointing at a pinned tag
      Cargo.toml
      src/lib.rs            # deliberately leave a testable failure point
  cases/
    case-001.toml           # case metadata: description, working directory, scoring command
  scripts/
    grade.sh                # single entry point: prepare the environment → run scoring → emit JSON
```

### 2.2 Example metadata for a single case (`case-001.toml`)

The fields are examples only; rename them to fit your team's habits:

```toml
id = "case-001"
description = "Fix add() off-by-one"

[workspace]
# During scoring, copy the fixture to this temp directory to avoid polluting the fixture source
template = "fixtures/tiny-rust-bug"

[[grade]]
command = ["cargo", "test", "-q"]
cwd = "."
expect_exit_code = 0
```

### 2.3 Key points of the `grade.sh` logic

1. `tmpdir=$(mktemp -d)`, and **rsync/cp** `fixtures/...` into `$tmpdir`.  
2. If the case description says "the model should produce a patch", first apply the **golden patch** or the **agent output patch** to `$tmpdir` (`git apply` or `patch -p1`).  
3. Run `grade.command` inside `$tmpdir`, and check the **exit code** and (optionally) a **stdout substring**.  
4. Print a single JSON line: `{"id":"case-001","pass":true}` so CI can collect it.

**Benefit**: fully **A-scored**; does not depend on the Pointer UI; Docker is optional.  
**Relation to Pointer**: this layer verifies "whether the task definition is reasonable"; for wiring in the Agent see §4 Wiring into Pointer: drive Coder with a local Web Server + API.

---

## 3. Pin the Runner with Docker (an enhanced B)

When the dependencies are **multiple Node versions / system libraries**, or you want to avoid polluting the host machine:

1. Write a `Dockerfile` for `fixtures/tiny-rust-bug`: pin `FROM rust:1.xx-bookworm`, and install only the tools **needed for scoring**.  
2. `docker build -t pointer-eval:case001 .`  
3. `docker run --rm -v "$PWD/out:/out" pointer-eval:case001 cargo test -q`

**Relation to SWE-bench**: SWE-bench is also "one container per instance + tested"; your in-house cases can keep the same habit, so the mental model stays consistent when you later migrate to the official SWE harness. Reference: [SWE-bench](https://github.com/swe-bench/swe-bench).

---

## 4. Wiring into Pointer: drive Coder with a local Web Server + API

The main repository already provides **HTTP + SSE** (see `server/src/main.rs`), which suits writing scripts that drive multi-turn conversations (you need to consume the SSE to parse the final message and tool results).

### 4.1 Start the local Core

```bash
cd /path/to/pointer-app
npm install
npm run server:dev
```

(The same as the Web development flow in the README; the port is whatever the terminal prints—below we assume `http://127.0.0.1:3000`.)

### 4.2 Configure the API and model

1. Open the Web UI in a browser, or call `PUT /api/settings` (see the server routes), to point **`workspaceRoot`** at the **temporary copy from §2 Minimum viable setup (recommended first step): "golden task + scripted scoring"** or a dedicated small repository.  
2. **Cloud model**: `POST /api/key` to store the key; or  
3. **Local model (C)**: in Settings set the **Base URL** to e.g. `http://127.0.0.1:11434/v1` (Ollama's OpenAI-compatible endpoint), and fill in a model name that already exists in Ollama.

### 4.3 Start one conversation turn

- **Endpoint**: `POST /api/chat`, with the request body `SendChatPayload` (`crates/pointer-core/src/models.rs`):  
  - `conversationId`: a fixed test conversation id.  
  - `messages`: a `ChatMessage` array (at least one user message; the fields match the frontend, including `role`/`content` etc.).  
  - `agentMode`: set to the mode string for coder (the same as when the frontend selects Coder; you can check the list and ids via `GET /api/agents`).  
  - `enabledSkillIds` / `toolRoundsUsed`: pass as needed, matching production.

- **Streaming results**: `GET /api/chat/:conversation_id/stream` (SSE); the script must parse events until `Done` or the end of the tool round, then extract from the persisted conversation or the events **whether the final workspace was modified**, and go back to **the `grade.sh` of §2 Minimum viable setup (recommended first step): "golden task + scripted scoring"** for scoring.

**Note**: the repository does **not yet** have a ready-made "one CLI command runs the whole eval" command; automation usually means writing your own **small Python/Node script** that calls the API + SSE above. If an official runner is added under `examples/` or `eval/` in the future, it can be a follow-up implementation item (see [`coder-agent-capability-roadmap.md`](../../zh-CN/design/coder-agent-capability-roadmap.md) §5.11 L — offline evaluation set).

### 4.4 Tool approval

If sensitive tools require approval in Settings, use `POST /api/tools/:tool_call_id/approve`. The script must automatically send `approve: true` after receiving an "awaiting approval" style event, to achieve unattended regression runs.

---

## 5. Scoring strategy checklist (easy to hard)

| Scoring type | How | When it fits |
| --- | --- | --- |
| Exit code | `cargo test`, `npm test` | First choice |
| Output substring | `grep -q` on stdout/stderr | Quick assertion |
| File content | `rg` / `test -f` / `sha256sum` compared against golden | Check whether the right file was changed |
| Forbidden changes | `git diff --name-only` is empty for paths that must not be touched | Prevent going off-task |

---

## 6. Relation to CI

- **Not mandatory**: it is enough that the local `eval/scripts/grade.sh` runs through.  
- **Optional**: add a job in GitHub Actions: cache the Docker layers, and run only a **Lite subset** (say 3–5 cases) to avoid blocking the main CI.

---

## 7. FAQ

**Q: Can I test the Agent without an API key?**  
A: You can test the **scoring chain of §2 Minimum viable setup (recommended first step): "golden task + scripted scoring"**; to test real model calls you need a **local model (C)** or an intranet gateway, otherwise a key is still required.

**Q: Can it be fully air-gapped?**  
A: The **scoring script** can; a **cloud LLM** cannot. A full air-gap requires **C**.

**Q: Is it either SWE-bench or this?**  
A: They are not mutually exclusive. Your in-house `eval/` handles **your product regressions**; SWE-bench handles **cross-cutting benchmarking**; for the latter see roadmap §5.11 L — offline evaluation set and the earlier discussion.

---

## 8. Maintenance advice

- Use a **submodule pinned to a tag** or a **minimal vendored tarball** for fixtures, so that "upstream main changes and the case drifts" does not happen.  
- Give each case a **timeout** in CI (say a 15–30 minute cap), to prevent a model loop from hanging the runner.

---

**Related docs**: [`coder-agent-capability-roadmap.md`](../../zh-CN/design/coder-agent-capability-roadmap.md) (§5.11 L — offline evaluation set, metrics §7)
