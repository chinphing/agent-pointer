# 任务板（task_board）与验证门禁

本文档面向维护者，说明「多步任务可观测 + 工具协议侧车」的设计与实现落点；**不是**运行时提示词。

## 聊天 UI 任务板面板

- 会话消息列表上方 **`TaskBoardPanel`**（可折叠）：展示当前会话 **parent** 板 `goal`、里程碑状态与子板摘要。
- 数据：`GET` / Tauri **`get_task_board_snapshot`**；流式 **`task_board_updated`**（`task_board` 工具成功或 Supervisor 规划同步后）。
- Agent **`AGENT.md`** 的 **`ui.showTaskBoardPanel`** / **`ui.hideToolNames`** 控制面板与工具卡展示（见 `docs/ui/visual-theme.md` 同目录的 agent `ui` 约定）。

## 目标

- 在较长对话中减少「做到哪了、凭什么算过」丢失：由宿主维护 **`task_board`** 状态，并在每轮末尾以公共 user 动态块注入任务板摘要（有内容时）。
- 允许同一轮在跑 **`terminal` / `file` / 桌面工具`** 前，先批量执行白名单 **侧车** 管理调用（首版为 **`task_board`**），而不把根级协议改成「多组并列根 `tool_name`」。

## 工具：`terminal`（取消与强制结束）

- 用户 **停止生成** 或宿主对会话 **`cancel`** 时，正在执行的 **`terminal`** 子进程会被 **终止**（与无输出超时、墙钟上限触发的终止共用同一套子进程清理逻辑；Windows 上对 shell 使用 `taskkill /T /F` 等，见 `crates/pointer-core/src/tools/terminal.rs`）。
- 工具 JSON 结果中会包含 **`cancelled`: true**（以及流式输出末尾的说明），便于区分「自然退出 / 超时」与「用户中断」。
- **仅结束终端命令、不停止本轮对话**：宿主可调用 **`AppState::abort_terminal_command(conversation_id, tool_call_id)`**（桌面 Tauri 命令 **`abort_terminal_command`**，可选 `toolCallId`；网页服务端 **`POST /api/chat/:conversation_id/abort-terminal`**，body `{ "toolCallId": "…" }`，响应体 **`{ "aborted": boolean }`**）。未传 `toolCallId` 时终止该会话全部 in-flight terminal；传入时仅终止对应工具卡。此时子进程同样被清理，但 **`CancellationToken` 不触发**；工具结果里为 **`runAborted`: true**（与 **`cancelled`** 区分）。同一会话在 **`terminal` 执行中** 时，前端工具卡可提供「结束命令」按钮调用该路径。

## 工具：`task_board`

- 注册名：`task_board`；行为通过 **`task_board:replace`** / **`task_board:patch`**（与 qualified `tool_name` 解析一致）。
- 存储：`AppState` 上的 **`TaskBoardStore`**（`crates/pointer-core/src/task_board/`，内存软缓存 + SQLite `{app_data}/task_boards.db`，按 **存储键** 分区）。有持久化时：`completed`/`failed` 写后即卸内存；空闲 **120 分钟**卸；缓存上限 **20**，超出 LRU。卸缓存不影响盘上数据与 UI（下次 `ensure_loaded` 回源）。无持久化（单测）不驱逐。v4 文档见 [`task-board-v2-schema.md`](task-board-v2-schema.md)；父子协调见 [`task-board-parent-child-coordination.md`](task-board-parent-child-coordination.md)。
- **主会话（单智能体 / Supervisor 主消息）**：存储键通常为 **`main_turn_task_board_store_key(conversation_id, anchor_user_message_id)`**（见 `session_inner`）；`task_board` 的 **`_conversation_id`** 写入该 **store key**（非裸 `conversation_id`）。每轮由 **`CommonUserDynamicInjectHook`** 在 `message_loop_prompts_after` 末尾追加 user 注入块（Markdown v4：`## Task` / `## Global milestones` / `done_when` / `remark` 等，有 board 或 init hint 时）。见 **[`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)**。
- **Supervisor 子 Agent**：与主会话 **隔离**。存储键为  
  **`{conversation_id}\x1fptr_sub_agent\x1f{supervisor_task_id}`**（实现见 `task_board::sub_agent_task_board_store_key`）。  
  子 Agent 的任务板摘要同样经公共 user 注入路径注入（store key 为 `sub_task_board_key`）；**`task_board`** 读写只针对该子任务键，**不会**看到或修改主会话任务板。
- **可信会话键**：宿主在 `invoke` 前写入 **`_conversation_id`**，覆盖模型可能传入的同名字段，防止伪造；子 Agent 路径下写入的是上述 **子任务键**，不是裸 `conversation_id`。
- 侧车标记：注册为 **`ToolEntry::new_sidecar`**（宿主侧 **`validate_envelope_tool_batch`** 等约束）；用法与 **`response` / `<sidecar_tools>`** 约定见 **`COMMUNICATION_PUBLIC`** 及各工具 **`doc_markdown`**（经 **`generate_tools_system_appendix`** 进入系统提示中的 **`## Tools`**）。未授权该工具时不会出现在上述附录中。
- **不再**在空 board 时注入 **`[TASK_BOARD_HINT]`**（该块曾放在 ephemeral `injected_tail`，每轮工具回合都会重打）。有内容时仍注入 live **`[TASK_BOARD]`** 快照；init 规则以 **`task_board`** 工具文档与 Coder / Computer 提示词为准。

## XML：`<sidecar_tools>` + 根级主工具

- 在 **`<response>`** 内，**可选** **`<sidecar_tools>`**，其中零或多个 **`<call>`**，每个子结构与单工具相同（**`tool_name` / `tool_args`**）。
- **根级仍恰好一对** **`tool_name` / `tool_args`**，表示本回合 **主工具**。
- **执行顺序**：先顺序执行所有侧车 **`call`**，再执行根级主工具（与产品约定一致）。
- **宿主校验**（`validate_envelope_tool_batch`）：若一轮解析出 **多条** `ToolCall`，则除最后一条外必须均为 **侧车工具**；最后一条 **不得** 为仅侧车工具。非法组合不执行本批，并注入环境反馈消息让模型重试（占用工具轮次预算，与空工具 XML 重试类似）。

## 系统提示：工具文档与侧车约定

- 各工具的详细说明来自其 **`doc_markdown`**（通常 `include_str!("prompts/…")`），与授权列表一起在 **`generate_tools_system_appendix`** 中拼入系统提示（**`## Tools`** 等）。
- **`task_board/prompts/task_board.md`**（英文，经 **`## Tools`** 附录）：多步计划的 **字段、patch 节奏、示例** 以 tool doc 为准。
- **`COMMUNICATION_PUBLIC`**（英文）：native tool calling、web 引用、skills、语言等跨 profile 规则；**不含** task board 操作细节。
- **桌面** **`tool_args.wait`** 见各 desktop 工具 `prompts/*.md` 与 **`computer/prompts/tiers/primary/communication.md`**（加载/转场时优先 `wait`）；**Coder** 专属的 **Definition of done** 与 **Cross-surface verification** 见 **`coder/COMMUNICATION.md`**。

## Agent 白名单

- 在对应 **`AGENT.md`** 的 **`accessPolicy.allowTools`**（及 computer 的 **`toolNames`** 若使用）中加入 **`task_board`**，模型才会在提示中看到该工具并合法调用。
- Supervisor 主流程本身不跑子 Agent 工具循环；子 Agent 各自按上表授权。
- 子 Agent 的 **`task_board`** 与主会话 **分区存储**（见上文「Supervisor 子 Agent」）；若需要把主会话进度写进子任务，由 Supervisor 在 **`instruction`** 文本中自行摘要，而不是共享存储键。

## 任务粒度与 v4 验收字段

- 板上一行应对应 **可独立验收** 的里程碑；**`done_when`** 写清 outcome 验收标准；完成时可选 **`remark`** 写证据摘要。
- Init / 行形状：以 **`task_board/prompts/task_board.md`** 与 Coder / Computer 侧提示为准（不再另写一套量化门禁；也不再注入 empty-board hint）。
- **`action_verify`** 仅用于 **单步** UI/操作校验，与板字段 **`done_when` / `remark`** 不同名、不同语义。
- **`done`** 建议在有 **`remark`**、近期 action tools 或（Type2）`work_item_delta` + `result_summary` 后更新（宿主可 warn `done_without_action`）。
- 对 computer 路径建议统一时序：首轮 `init` 可无 `action_verify`；其后 **`action_verify`（步）→ `task_board_patch`（里程碑）**。
- 里程碑粒度建议（跨入口统一）：
  - **矩阵/组合类任务**：优先做 3–8 个分组里程碑，按交互形态/维度分组。
  - **列表类任务（Type2）**：枚举清单在 **`work_items`** + **`item_milestones`** 模板；global 固定 `g_plan` / `g_exec` / `g_deliver`。

## 与压缩上下文的关系

- 每轮末尾注入任务板摘要可降低任务板只存在于旧 tool 消息里被压掉的风险。
- Prompt 注入坚持最小必要：快照优先保留当前执行行、可就绪后续行与已完成摘要，长 `detailed_plan` 在快照中会被截断。
- 若后续在 **`context_compression`** 中增加高保留信号，可将 **`TASK_BOARD` / `task_board`** 输出纳入优先级（可选增强）。

## 内存预算与自动瘦身

- 行状态首次进入 `done` 时，宿主会清空该行 **`plan`**（v4；里程碑证据在 **`remark`**）以减少后续 token 压力。
- 若 `global_context.artifacts.interim_drafts` 超过预算阈值，宿主会对超长草稿做截断并在 `warnings` 中返回 `interim_drafts_budget_exceeded`，同时设置 `reflection_required=true`，提示下一轮做摘要化整理。

## 灰度与观测建议

- 阶段 A（提示词）：关注前 3 轮内 `task_board_init` 命中率、`action_verify -> task_board_patch` 时序合规率。
- 阶段 B（主会话 hint）：观察 `main_agent_init_hint` 触发后初始化成功率、误触发率（单步任务）。
- 阶段 C（软门禁增强）：跟踪 `done_without_action` / `g_exec_not_terminal` 占比和 `reflection_required` 收敛速度。
- Token 成本指标：单轮 prompt tokens、单任务累计 tokens、history trim 后回落幅度。
- 双入口一致性：桌面端与 Web 端都应收到 `task_board_updated` 且面板状态一致。

## task_board 触发的历史截断（当前实现）

在 **`task_board`** 变更满足 **trim 触发** 条件且工具执行成功后，对已启用该能力的 Agent 可对会话 history 做 **soft-exclude**（`context_state.included = false`，`ExcludedReason::TaskBoardTrim`；**不**调用 LLM 摘要），与 [`context_compression`](../crates/pointer-core/src/context_compression.rs) 互补。实现：`task_board/history_trim.rs`，挂载：`agent_tool_pass.rs`。

**Trim 触发条件（`task_board/checkpoint.rs`，v4）：**

- **`task_board_init` / `replace` / `finalize`** 成功 → 触发。
- **`task_board_patch`** 成功 → 本批含实质进展：行 `status: done`、非空 **`remark`**、或顶层 **`work_item_delta`**。
- 仅 **`in_progress`** 且无 remark / work_item_delta → 不触发。
- v3 字段（`progress`、`validate_results` 等）→ **不**触发。
- 仍参与 LLM 上下文的消息数 **&lt; 10** → 不截断（`MIN_INCLUDED_MESSAGES_FOR_TRIM`）。

| 机制 | 触发 | 处理方式 | 成本 |
|------|------|----------|------|
| task_board 阶段截断 | 上表 + 工具成功 + Agent 开关 on | soft-exclude（UI 仍可见） | 无 LLM |
| context compression | 字符预算 / 工具轮次上限 | 较早前缀 LLM 摘要 | 额外 API |

**按 Agent 策略：**

| Agent | 默认开关 | 保留集合 |
|-------|----------|----------|
| `computer` | on | **绑定 anchor user**（`get_main_task_board_anchor`）+ **最近 10 条消息** + **最新 live `[CUR_SCREEN]`**（非 placeholder）；其余 exclude |
| 其它（`coder` 等） | off | 若启用：按 **user 边界**保留首条真实 user + 末尾 K=2 个 user 的 suffix（`trim_history_after_task_board`） |

- **配置：** 设置里 **按智能体** `agentTaskBoardHistoryTrim`（`ModelSettings`）。
- **计划状态：** 每轮 **`CommonUserDynamicInjectHook`** 末尾注入 Markdown **`[TASK_BOARD]`**（见 [`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)）；不依赖被 exclude 的旧 tool 正文。
- **绑定：** main-turn store key 与 **anchor user message id** 见 `session_inner::choose_main_task_board_store_key`、`app_state::set_main_task_board_binding`。

### v4 patch 与 Computer Type2（提示）

- **Type1 / coder：** patch **`global_milestones`**（len=1）；`done` 时写 **`remark`**。
- **Type2：** patch **`milestones`**（item SOP）+ **`work_item_delta`**；交付在 **`g_deliver`** + **`work_items_export`**。
- **Computer init（提示词）：** 枚举重复项 **>5** 用 Type2（`g_plan`/`g_exec`/`g_deliver` + `item_milestones` + `work_items`）；见 `task_board.md`、`sub_agent_hint.rs`。

### 与 LLM 压缩的执行顺序

1. 本回合 tool batch 结束 → 若 task_board trim checkpoint 满足且开关 on → soft-exclude。
2. 下一轮若仍超 `contextBudgetChars` → `maybe_compress_history`（LLM 摘要）。

---

## 待实现：checkpoint 触发 + 仅保留当前轮 screen（暂缓，未编码）

**状态：** 仅设计记录；**暂不实现**（同轮 tool loop、看板完整度、步级 verify 等风险未收敛）。

### 提案摘要

1. **触发：** 将 trim 的「阶段节点」从 **`patch` + `status: done`** 改为 **`patch` 导致当前行 `checkpoint` 在 store 中发生变化**（`init` / `replace` / `finalize` 仍可触发）。与 v3「`checkpoint` = 粗粒度 phase/cycle 切换」对齐。
2. **保留：** 对 Computer（或仅 trim-on 的 agent）在触发时 **exclude 几乎全部 history**，只保留 **当前轮最新 live `[CUR_SCREEN]`** 注入（含图）；不再保留 anchor user、最近 10 条、本轮 assistant/tool。

### 预期收益

- 长周期 `in_progress` 里程碑内即可大幅降 token，不必等 `done`。
- 强制「看板 + 当前桌面」双锚，缓解历史与 `[CUR_SCREEN]` 正文膨胀。

### 主要风险（实现前需产品/协议确认）

| 风险 | 说明 |
|------|------|
| **同轮 tool loop 断裂** | trim 在 `agent_tool_pass` **工具批之后**执行；若只留 screen，会 exclude **本轮**紧随其后的 assistant、`tool` 结果。若同一轮继续 `stream_chat`，模型可能看不到刚执行的 verify/桌面工具输出。 |
| **anchor 用户原文丢失** | 不保留绑定 user 行；长需求仅在 `meta.goal` 写得全时才安全。 |
| **步级证据丢失** | 未写入 `remark` / `work_item.result_summary` 的 `action_verify` / 工具原文在 trim 后不可恢复。 |
| **checkpoint 过频（已废弃）** | v4 已移除 `checkpoint` / `progress` 触发；见 v4 trim 规则。 |
| **screen 仍很大** | 只留一条 user 仍可能占满预算（多图 + Advanced 全表 bbox + tier history）。 |
| **消息序列** | API 过滤后可能仅剩一条 user，需验证与多轮 tool 循环、Computer 展平逻辑的兼容性。 |

### 实现前建议采用的变体（择一）

- **A（提案字面）：** 仅最新 live `[CUR_SCREEN]` — 最简单，风险最高。
- **B（推荐）：** `checkpoint` 变化触发；保留 **最新 `[CUR_SCREEN]` + 从该条到 history 末尾**（含本轮 assistant/tool）。
- **C（更稳）：** 在 B 上 **额外保留 anchor user 一行**（可截断）。

### 配套（若将来实现）

- Computer 提示词：SOP 步完成或 work_item 完成时 **patch `remark` / `work_item_delta`**，再视情况触发 trim。
- `checkpoint.rs`：apply 后 diff `checkpoint`，而非仅解析 args。
- 观测：`task_board_trim` 日志区分触发原因（`done` vs `checkpoint_changed`）、exclude 条数、下轮 prompt token。

**相关讨论记录：** 维护者对话 2026-06（checkpoint trim + screen-only 评估，结论为暂缓）。

## 相关代码入口（维护索引）

- 公共提示词：`crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md`
- 工具 XML 说明：`crates/pointer-core/src/tools/prompts/response.md`
- 侧车解析与多 `ToolCall`：`crates/pointer-core/src/tool_envelope.rs`、`crates/pointer-core/src/json_tool_caller.rs`、`crates/pointer-core/src/provider.rs`
- 批校验与工具注册：`crates/pointer-core/src/tools/mod.rs`
- 会话注入与执行：`crates/pointer-core/src/chat_service/`（主流程 `session_inner.rs`，单智能体 `single_agent.rs` + 薄封装，子 Agent `sub_agent.rs` + `sub_agent_prompt.rs` / `sub_agent_stream.rs`，共用 `agent_stream_round.rs` / `agent_post_stream.rs` / `agent_tool_pass.rs`）；任务板快照钩子：`crates/pointer-core/src/extensions/task_board_hook.rs`
- task_board 阶段截断：`task_board/history_trim.rs`、`message_context.rs`（`find_split_at_user_boundary`）、`agent_tool_pass.rs`（挂载点）
- 父子 Gateway：`task_board/gateway/`、`chat_service/supervisor.rs`（`dispatch_to_child` / `report_child_status`）
- Computer 每轮 user 注入：`crates/pointer-core/src/agents/computer/extension_hooks/screen_inject.rs`；API 展平：`models.rs`（`flatten_tool_rounds_computer_style_for_api`）
