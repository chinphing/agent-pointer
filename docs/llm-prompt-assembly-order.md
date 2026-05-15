# LLM 请求中的提示词与消息顺序

本文说明主对话 / 子 Agent 在调用 `OpenAIProvider::stream_chat` 时，**送入模型的 `messages` 与 `system_prompts` 如何拼在一起**。扩展钩子总览见 **`docs/agent-extension-hooks.md`**。

> **行号**：下文中的行号便于在仓库内检索；若你本地分支与主分支不一致，请以 **符号名**（函数 / 结构体）为准，用 IDE 或 `rg` 定位。

---

## 1. `stream_chat` 主路径（单智能体与子 Agent 同构）

### 1.1 `messages`（`history_for_api`）

| 步骤 | 行为 | 参考代码 |
|------|------|----------|
| 克隆 | `history.clone()`（主会话）或 `local_history.clone()`（子 Agent） | `crates/pointer-core/src/chat_service.rs` — `run_chat_inner` / `run_sub_agent` 内 `let mut history_for_api = …` |
| 同轮扩展 | `run_message_loop_prompts_after`：在克隆的 `messages` 上追加（如 Computer **`user` + `[CUR_SCREEN]`**） | `chat_service.rs` 中 `run_message_loop_prompts_after`；Computer 见 `crates/pointer-core/src/agents/computer/extension_hooks/screen_inject.rs` |

**说明**：`messages` **不含**尾随的 `[Environment]` user；环境信息在 **§1.2** 的 `system_prompts` 末尾。

### 1.2 `system_prompts`（合并为 HTTP 首条 `role: "system"`）

在 `run_before_main_llm_call` **之前** 依次 `push` / `extend`；**钩子**向同一 `Vec` 追加；**钩子返回之后**再由 `chat_service` 追加 **`[Environment]`**：

| 顺序 | 内容 | 参考代码 |
|------|------|----------|
| 1 | **公共 COMMUNICATION** | `rendered_communication_public_inject()` |
| 2 | **Agent 系统提示**（`AGENT.md` + profile `COMMUNICATION.md` 等，经 `expand_agent_prompt_placeholders`）；主会话含 agent 计划中的 prompts；子 Agent 另含 **sub_agent_header** + **skills** | `agent_plan.system_prompts` 或 `run_sub_agent` 内 `prompts` |
| 3 | **工具系统附录**（授权工具的 `doc_markdown` 等） | 非空时 `push(tools_system_appendix)`；`crates/pointer-core/src/tools_system_appendix.rs` **`generate_tools_system_appendix`** |
| 4 | **`[TASK_BOARD]` 等** | `run_before_main_llm_call`：`crates/pointer-core/src/extensions/task_board_hook.rs` 等钩子 `ctx.system_prompts.push(…)` |
| 5（最后） | **`[Environment]`**（`env_prompt::build_environment_system_prompt_slice`：OS、locale、**日历日期**） | **`push_env_context_last_in_system_prompts`**（`chat_service.rs`），在 **`run_before_main_llm_call` 的 `.await` 之后**调用，保证为合并 `system` 的**最后一段**（`join("\n\n")` 时排在末尾） |

### 1.3 HTTP `messages` 最终顺序（`make_openai_messages`）

| 顺序 | 角色 | 说明 |
|------|------|------|
| 1 | `system` | `system_prompts.join("\n\n")` — 即 **§1.2** 整表顺序拼成一条 |
| 2… | `user` / `assistant` / … | 对 **§1.1** 中的 `msgs` 先做 `expand_tool_messages_for_openai_request`，再 `flatten_tool_rounds_computer_style_for_api`，再按展平结果依次输出 |

| 参考代码 | 说明 |
|----------|------|
| `crates/pointer-core/src/models.rs` | `expand_tool_messages_for_openai_request`、`flatten_tool_rounds_computer_style_for_api`、`make_openai_messages` |
| `crates/pointer-core/src/provider.rs` | `stream_chat` / `chat_once` 调用 `make_openai_messages` |

---

## 2. 其他 API 路径（非上述 `stream_chat` 堆栈）

| 场景 | 说明 | 参考代码 |
|------|------|----------|
| Supervisor **规划** / **汇总** | 使用 `chat_once` + 独立 `system` 字符串模板；模板内可嵌入 **`env_prompt::build_environment_context_full()`**（含完整 **Local time**）；**不**走 `message_loop_prompts_after` / `before_main_llm_call` / `push_env_context_last_in_system_prompts` | `chat_service.rs` 中 `plan_agent_tasks`、`synthesize_final_answer` |

---

## 3. 提示词资产与 §1 的对应关系（清单）

| 类型 | 典型文件 / 位置 |
|------|----------------|
| **AGENT.md** | `crates/pointer-core/src/agents/<id>/AGENT.md`，并入 agent 的 `system_prompts` 条目 |
| **COMMUNICATION.md** | 同上目录；与 AGENT 等合并后经 `expand_agent_prompt_placeholders` → **§1.2 第 2 行**；Coder 含 **`read_lints`**、**Git** 等会话策略 |
| **COMMUNICATION_PUBLIC** | `crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md` → **§1.2 第 1 行** |
| **Tools** | `crates/pointer-core/src/tools/prompts/*.md`（共享内置工具）与 `crates/pointer-core/src/agents/coder/prompts/*.md`（仅 Coder 的工具，如 `read_lints`）等 → **`generate_tools_system_appendix`** → **§1.2 第 3 行** |
| **Task board** | `TaskBoardSnapshotHook` 等 → **§1.2 第 4 行** |
| **Env** | `env_prompt::build_environment_system_prompt_slice` + **`push_env_context_last_in_system_prompts`** → **§1.2 第 5 行**（**仅日历日期**）；Computer **`screen_inject`** 在 **`[CUR_SCREEN]`** 正文前加 **`format_local_wall_clock_full`**（**完整日期时间**）；Supervisor **`chat_once`** 用 **`build_environment_context_full`** |
| **屏幕等多模态** | `screen_inject.rs` → **§1.1**，`user` + 图，**不在** `system_prompts.join` 里 |
