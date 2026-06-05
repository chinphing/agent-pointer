# LLM 请求中的提示词与消息顺序

本文说明主对话 / 子 Agent 在调用 `OpenAIProvider::stream_chat` 时，**送入模型的 `messages` 与 system 提示如何拼在一起**。扩展钩子总览见 **[`agent-extension-hooks.md`](agent-extension-hooks.md)**。千问显式 Context Cache 见 **[`../llm/qwen-context-cache.md`](../llm/qwen-context-cache.md)**。

> **行号**：下文中的行号便于在仓库内检索；若你本地分支与主分支不一致，请以 **符号名**（函数 / 结构体）为准，用 IDE 或 `rg` 定位。

---

## 1. `stream_chat` 主路径（单智能体与子 Agent 同构）

### 1.1 `messages`（`history_for_api`）

| 步骤 | 行为 | 参考代码 |
|------|------|----------|
| 克隆 | `history.clone()`（主会话）或 `local_history.clone()`（子 Agent） | `session_inner.rs` / `single_agent.rs` / `single_agent_stream.rs` / `sub_agent.rs` — `run_chat_inner` / `run_single_agent_loop` / `run_sub_agent` 内 `let mut history_for_api = …`（子 Agent 在 `sub_agent_prompt.rs`） |
| 同轮扩展 | `run_message_loop_prompts_after`：在克隆的 `messages` 上追加（如 Computer **`user` + `[CUR_SCREEN]`**、公共 **user dynamic inject**） | `single_agent_prompt.rs` / `sub_agent_prompt.rs` 中 `prepare_*_round_prompts`；Computer 见 `crates/pointer-core/src/agents/computer/extension_hooks/screen_inject.rs`，公共注入见 `extensions/common_user_dynamic_inject_hook.rs` |

**说明**：`messages` **不含** `[Environment]` user；环境日期等在 **§1.2** cacheable 的 `[Environment]` 块中。

### 1.2 `SystemPromptSections`（cacheable + dynamic）

每轮组装为 [`SystemPromptSections`](../../crates/pointer-core/src/models.rs)（`cacheable` / `dynamic` 两个 `Vec<String>`），在 `make_openai_messages` 中序列化为 HTTP `system`。

| 分区 | 顺序 | 内容 | 稳定性 |
|------|------|------|--------|
| **cacheable** | 1 | **公共 COMMUNICATION** | `rendered_communication_public_inject()` | 固定 |
| | 2 | **Agent 系统提示**（`AGENT.md` + profile `COMMUNICATION.md` 等，经 `expand_agent_prompt_placeholders`）；子 Agent 含 **sub_agent_header** + **skills** | `agent_plan.system_prompts` 等 | 会话内固定（`{{workspace_root}}` 随工作区变） |
| | 3 | **工具系统附录** | `generate_tools_system_appendix` | 工具集不变则固定；同一 `ToolEntry::doc_source`（提示词 `.md` 路径）只输出一次 |
| | 4 | **`[Environment]`**（OS、locale、**日历日期**） | `push_env_to_cacheable` | 按自然日变，**非每轮** |
| **dynamic** | 5 | **`[LOCKED GOAL]`**（Computer 有锁时） | `before_main_llm_call` 钩子 → `system_prompts_dynamic` | **每轮可能变** |

**组装时机**

- **cacheable**：在 `run_before_main_llm_call` **之前** 填完（含 Environment）。
- **dynamic**：仅钩子写入（**`ComputerTierDynamicHook`**）。
- **user dynamic inject**：在 `run_message_loop_prompts_after` 末尾追加一条 `user`，承载 task board Markdown 与 JSON wire tail。
- **Computer lead**：`prepare_single_agent_round_prompts` 每轮按 **tier** 重建 cacheable 中的档位 communication（升档时缓存失效一次）。

合并为单条 system 字符串时，顺序为 **cacheable 全文 → dynamic 全文**；task board 与 JSON wire tail 已迁移到 `messages` 末尾的公共 user 注入块。

### 1.3 HTTP `messages` 最终顺序（`make_openai_messages`）

| 顺序 | 角色 | 说明 |
|------|------|------|
| 1 | `system` | 见 **§1.4** |
| 2… | `user` / `assistant` / … | `expand_tool_messages_for_openai_request` → `flatten_tool_rounds_computer_style_for_api` |

| 参考代码 | 说明 |
|----------|------|
| `crates/pointer-core/src/models.rs` | `SystemPromptSections`、`push_openai_system_messages`、`make_openai_messages` |
| `crates/pointer-core/src/provider.rs` | `stream_chat` / `chat_once` |

### 1.4 千问显式 Context Cache 序列化

当 **`qwen_explicit_system_cache_enabled(settings)`** 为真且 **cacheable** 非空时：

```json
{
  "role": "system",
  "content": [
    {
      "type": "text",
      "text": "<cacheable 各 slice 用 \\n\\n 合并>",
      "cache_control": { "type": "ephemeral" }
    },
    {
      "type": "text",
      "text": "<dynamic：通常仅 [LOCKED GOAL]>"
    }
  ]
}
```

- task board / JSON wire tail 在 `messages` 末尾追加，不会污染 system cacheable 前缀。
- **`[Environment]`** 仅在跨日时改变 cacheable（ acceptable）；同一天内多轮工具循环可复用 cacheable。
- 非千问或未启用时：两分区仍按 §1.2 顺序合并为单条 `content` 字符串。

官方说明：[千问 Context Cache](https://help.aliyun.com/zh/model-studio/context-cache)；应用细节见 [`qwen-context-cache.md`](../llm/qwen-context-cache.md)。

### 1.5 Native OpenAI `tools[]`（与 system 附录分工）

除 **§1.2 cacheable** 中的 **`## Tools`** 附录（按 `doc_source` dedup 一次）外，每轮还在 HTTP body 的 **`tools`** 字段发送 flat tool schema（`ToolRegistry::openai_tools`）。

| 字段 | 策略 |
|------|------|
| `function.parameters` | 每个 flat tool 独立 JSON Schema（不变） |
| `function.description` | 同一 `doc_source` 下多个 flat tool → **短描述**（含 tool 名 + 指向 system Tools appendix）；完整 `doc_markdown` **不再**在每个 flat tool 上重复 1024 字符 |
| 唯一 doc 且正文 ≤240 字符 | 仍可直接使用完整 `doc_markdown` |

参考：`crates/pointer-core/src/tools/mod.rs`（`openai_description_for_entry`）。Token 基线见 `scripts/count_prompt_tokens.py`。

---

## 2. 其他 API 路径（非上述 `stream_chat` 堆栈）

| 场景 | 说明 | 参考代码 |
|------|------|----------|
| Supervisor **规划** / **汇总** | `chat_once` + 独立 system 模板（可含完整 **Local time**） | `supervisor_plan.rs`、`supervisor_synth.rs`；`SystemPromptSections::all_cacheable` |
| **上下文压缩** 摘要 | `chat_once` + 固定摘要 system | `context_compression.rs` |

---

## 3. 提示词资产与 §1 的对应关系（清单）

| 类型 | 典型位置 | 分区 |
|------|----------|------|
| **COMMUNICATION_PUBLIC** | `agents/_shared/COMMUNICATION_PUBLIC.md` | cacheable |
| **AGENT.md** / **COMMUNICATION.md** | `agents/<id>/` | cacheable |
| **Tools** | `tools/prompts/*.md` 等 | cacheable |
| **Env** | `env_prompt::build_environment_system_prompt_slice` | cacheable（日历日期）；Computer **`[CUR_SCREEN]`** 含完整墙钟时间 |
| **JSON wire tail** | `_shared/JSON_WIRE_TAIL.md` | `message_loop_prompts_after` 的 user 注入 |
| **Task board** | `CommonUserDynamicInjectHook` | `message_loop_prompts_after` 的 user 注入 |
| **屏幕等多模态** | `screen_inject.rs` | **§1.1** `user` + 图 |

---

## 4. 迁移开关与回滚

- 开关：`userDynamicInjectEnabled`（`ModelSettings` / `PlatformSettings`，默认 `true`）。
- `true`：启用 `_99_common_user_dynamic_inject`，在每轮 `messages` 末尾注入 task board Markdown + JSON wire tail。
- `false`：回滚到旧路径（system cacheable 重新附加 JSON wire tail，system dynamic 重新附加 `[TASK_BOARD]` 快照 / hint）。
- 诊断日志：`common_user_dynamic_inject` 会输出 `legacy_snapshot_len` 与新 user 注入块长度，便于灰度对比。
