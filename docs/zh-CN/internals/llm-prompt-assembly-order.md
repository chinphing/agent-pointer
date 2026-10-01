# LLM 请求中的提示词与消息顺序

本文说明主对话 / 子 Agent 在调用 `OpenAIProvider::stream_chat` 时，**送入模型的 `messages` 与 system 提示如何拼在一起**。扩展钩子总览见 **[`agent-extension-hooks.md`](../developer/agent-extension-hooks.md)**。千问显式 Context Cache 见 **[`../llm/qwen-context-cache.md`](../llm/qwen-context-cache.md)**。

> **行号**：下文中的行号便于在仓库内检索；若你本地分支与主分支不一致，请以 **符号名**（函数 / 结构体）为准，用 IDE 或 `rg` 定位。

---

## 1. `stream_chat` 主路径（单智能体与子 Agent 同构）

### 1.1 `messages`（base + `injected_tail`）

| 步骤 | 行为 | 参考代码 |
|------|------|----------|
| 只读 base | 借用 `history`（主会话）或 `local_history`（子 Agent），**不**再整表 `clone` 进 API 快照 | `single_agent_prompt.rs` / `sub_agent_prompt.rs` — `prepare_*_round_prompts`；stream 侧 `build_stream_chat_wire` |
| 同轮扩展 | `run_message_loop_prompts_after`：仅向 **`injected_tail`** 追加（如 Computer **`user` + `[CUR_SCREEN]`**、公共 **user dynamic inject**） | Computer：`agents/computer/extension_hooks/screen_inject.rs`；公共：`extensions/common_user_dynamic_inject_hook.rs` |
| 组 wire | `make_openai_messages_with_inject(base, injected_tail, …)` → HTTP JSON；spawn 只持有 wire | `provider.rs` — `build_stream_chat_wire` / `stream_chat_wired` |

**说明**：API `messages` **不含** `[Environment]` user；环境日期等在 **§1.2 `SystemPromptSections`（cacheable + dynamic）** cacheable 的 `[Environment]` 块中。峰值内存说明见 [`long-chat-memory.md`](long-chat-memory.md)。

### 1.2 `SystemPromptSections`（cacheable + dynamic）

每轮组装为 [`SystemPromptSections`](../../../crates/pointer-core/src/models/openai_convert.rs)（`cacheable` / `dynamic` 两个 `Vec<String>`），在 `make_openai_messages` 中序列化为 HTTP `system`。

| 分区 | 顺序 | 内容 | 稳定性 |
|------|------|------|--------|
| **cacheable** | 1 | **公共 COMMUNICATION** | `rendered_communication_public_inject()` | 固定 |
| | 2 | **文件交付（`MEDIA:`）** | `rendered_media_delivery_inject()` → [`agents/_shared/MEDIA_DELIVERY.md`](../../../crates/pointer-core/src/agents/_shared/MEDIA_DELIVERY.md) | **仅 general / coder / computer** |
| | 3 | **图表（`chartjs`）** | `rendered_charts_inject()` → [`agents/_shared/CHARTS.md`](../../../crates/pointer-core/src/agents/_shared/CHARTS.md) | **仅 general / coder / computer** |
| | 4 | **SVG 图示（`svg`）** | `rendered_svg_diagrams_inject()` → [`agents/_shared/SVG_DIAGRAMS.md`](../../../crates/pointer-core/src/agents/_shared/SVG_DIAGRAMS.md) | **仅 general / coder / computer** |
| | 5 | **Agent 系统提示**：Computer 为 **tier** communication + loop（`push_agent_role_cacheable_prompts`，主轮与子 Agent 共用）；非 Computer 为 `AGENT.md` + profile 通信；子 Agent 另在步骤 5 后追加短 **sub_agent_header** + **skills** | `agent_plan.system_prompts` / tier 运行时 | 会话内固定（`{{workspace_root}}` 随工作区变） |
| | 6 | **工具系统附录** | `generate_tools_system_appendix` | 工具集不变则固定；同一 `ToolEntry::doc_source`（提示词 `.md` 路径）只输出一次 |
| | 7 | **`[Environment]`**（OS、locale、**日历日期**） | `push_env_to_cacheable` | 按自然日变，**非每轮** |
| | 8 | **`[MEMORY]` / `[USER PROFILE]`**（跨会话 frozen snapshot） | `memory::push_memory_to_cacheable` | 会话内冻结；**压缩成功后 reload** |
| | 9 | **`[USER RULES]`**（用户编码偏好，`userCodingRules`） | `user_rules::push_user_coding_rules_to_cacheable` | 用户改 settings 后下一会话生效 |
| | 10 | **`# Project Context`**（`AGENTS.md` 链） | `agents_md::push_agents_md_to_cacheable` | 随工作区与文件内容变；**不是** user 消息 |
| **dynamic** | — | **`[LOCKED GOAL]`**（Computer 有锁时） | `before_main_llm_call` 钩子 → `system_prompts_dynamic` | **每轮可能变** |

**组装时机**

- **cacheable**：在 `run_before_main_llm_call` **之前** 填完（含 Environment）。
- **dynamic**：仅钩子写入（**`ComputerTierDynamicHook`**）。
- **user dynamic inject**：在 `run_message_loop_prompts_after` 末尾追加一条 `user`，承载 task board Markdown（有内容或 init hint 时）。
- **Computer lead**：`prepare_single_agent_round_prompts` 每轮按 **tier** 重建 cacheable 中的档位 communication（升档时缓存失效一次）。

合并为单条 system 字符串时，顺序为 **cacheable 全文 → dynamic 全文**；task board 已迁移到 `messages` 末尾的公共 user 注入块（仅在有 board 内容或 init hint 时追加）。

### 1.3 HTTP `messages` 最终顺序（`make_openai_messages_with_inject`）

| 顺序 | 角色 | 说明 |
|------|------|------|
| 1 | `system` | 见 **§1.4 千问显式 Context Cache 序列化** |
| 2… | `user` / `assistant` / … | scope-filtered **base** history，再追加本轮 **`injected_tail`**，然后 `expand_tool_messages_for_openai_request` → `flatten_tool_rounds_computer_style_for_api` |

| 参考代码 | 说明 |
|----------|------|
| `crates/pointer-core/src/models/openai_convert.rs` | `SystemPromptSections`、`push_openai_system_messages`、`make_openai_messages_with_inject` |
| `crates/pointer-core/src/provider.rs` | `build_stream_chat_wire` / `stream_chat_wired` / `chat_once` |
| [`long-chat-memory.md`](long-chat-memory.md) | 为何不再 `history.clone()` 进 spawn |

### 1.4 千问显式 Context Cache 序列化

当 **`qwen_explicit_system_cache_enabled(settings)`** 为真且 **cacheable** 非空时，打最多两个 `cache_control`（官方单请求最多 4 个）：

1. **system cacheable**（原有）：稳定人设单独成块。
2. **对话历史最后一条**（新增）：`content` 改为数组并打标记，前缀随历史增长。
3. 其后才是本轮 **`injected_tail`**（`[CUR_SCREEN]` / 任务板等），不打标记。

```json
[
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
  },
  { "role": "user", "content": [{ "type": "text", "text": "<历史最后一条>", "cache_control": { "type": "ephemeral" } }] },
  { "role": "user", "content": "<injected_tail，例如任务板>" }
]
```

无对话历史时只有标记 1。非千问或未启用时：两分区合并为单条 `content` 字符串，历史也不打标记。

详见 [`qwen-context-cache.md`](../llm/qwen-context-cache.md) 与 [官方 Context Cache](https://help.aliyun.com/zh/model-studio/context-cache)。

### 1.5 Native OpenAI `tools[]`（与 system 附录分工）

除 **§1.2 `SystemPromptSections`（cacheable + dynamic） cacheable** 中的 **`## Tools`** 附录（按 `doc_source` dedup 一次）外，每轮还在 HTTP body 的 **`tools`** 字段发送 flat tool schema（`ToolRegistry::openai_tools`）。

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
| **上下文压缩** 摘要 | `chat_once` + 固定摘要 system | `context_compression/` |

---

## 3. 提示词资产与 §1 `stream_chat` 主路径（单智能体与子 Agent 同构）的对应关系（清单）

| 类型 | 典型位置 | 分区 |
|------|----------|------|
| **COMMUNICATION_PUBLIC** | `agents/_shared/COMMUNICATION_PUBLIC.md` | cacheable |
| **MEDIA_DELIVERY** | `agents/_shared/MEDIA_DELIVERY.md` | cacheable（仅 general / coder / computer） |
| **CHARTS** | `agents/_shared/CHARTS.md` | cacheable（仅 general / coder / computer；与 agent body 解耦） |
| **SVG_DIAGRAMS** | `agents/_shared/SVG_DIAGRAMS.md` | cacheable（仅 general / coder / computer；与 agent body 解耦） |
| **AGENT.md** / **COMMUNICATION.md** | `agents/<id>/` | cacheable |
| **Coder / Explore compose** | `agents/coder/mod.rs`, `agents/explore/mod.rs` → `composed_system_body()` | cacheable（`load_builtin_agent` 替换 AGENT 正文） |
| **Tools** | `tools/prompts/*.md` 等 | cacheable |
| **Env** | `env_prompt::build_environment_system_prompt_slice` | cacheable（日历日期）；Computer **`[CUR_SCREEN]`** 含完整墙钟时间 |
| **Task board** | `CommonUserDynamicInjectHook` | `message_loop_prompts_after` 的 user 注入（有 board 或 hint 时） |
| **AGENTS.md** | `agents_md::push_agents_md_to_cacheable` | cacheable **`# Project Context`** |
| **屏幕等多模态** | `screen_inject.rs` | **§1.1 `messages`（base + `injected_tail`）** `user` + 图 |

### 3.1 Coder / Explore `composed_system_body()` 顺序

`load_builtin_agent` 在解析 manifest 后替换 `AGENT.md` 正文（`compose_system_prompt(COMMUNICATION, body)` 不变）。

**Explore**（[`explore/mod.rs`](../../../crates/pointer-core/src/agents/explore/mod.rs)）：

1. `prompts/role.md`
2. `prompts/flow/router.md`, `standard.md`, `fast_narrow.md`, `fast_reachability.md`
3. `prompts/scenarios/*`（7 个 playbook）
4. `_shared/exploration/impact_scan.md`, `handoff_contract.md`, `trace_when.md`, `file_discipline.md`
5. `prompts/deliverable.md`

**Coder**（[`coder/mod.rs`](../../../crates/pointer-core/src/agents/coder/mod.rs)）：

1. `prompts/role.md`
2. `prompts/flow/routine.md`（G1/G2/G3 + Orient/Change/Check/Deliver）
3. `prompts/delegation.md`（含 handoff 合并摘要；**不** include 完整 `impact_scan.md`）
4. `prompts/task_board.md`
5. `prompts/scenarios/*`（5 个 playbook）

切片之间以 `\n\n---\n\n` 连接（`join_agent_prompt_sections`）。维护索引：[`docs/agents/coder-explore-prompts.md`](../agents/coder-explore-prompts.md)。

---

## 4. 迁移开关与回滚

- 开关：`userDynamicInjectEnabled`（`ModelSettings` / `PlatformSettings`，默认 `true`）。
- `true`：启用 `_99_common_user_dynamic_inject`，在每轮 `messages` 末尾注入 task board Markdown（有内容或 init hint 时）。
- `false`：回滚到旧路径（system dynamic 重新附加 `[TASK_BOARD]` 快照 / hint）。
- 诊断日志：`common_user_dynamic_inject` 会输出 `legacy_snapshot_len` 与新 user 注入块长度，便于灰度对比。
