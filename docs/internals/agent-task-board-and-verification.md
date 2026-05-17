# 任务板（task_board）与验证门禁

本文档面向维护者，说明「多步任务可观测 + 工具协议侧车」的设计与实现落点；**不是**运行时提示词。

## 目标

- 在较长对话中减少「做到哪了、凭什么算过」丢失：由宿主维护 **`task_board`** 状态，并在每轮系统上下文中注入 **`[TASK_BOARD]`** 快照（有内容时）。
- 允许同一轮在跑 **`terminal` / `file` / 桌面工具`** 前，先批量执行白名单 **侧车** 管理调用（首版为 **`task_board`**），而不把根级协议改成「多组并列根 `tool_name`」。

## 工具：`terminal`（取消与强制结束）

- 用户 **停止生成** 或宿主对会话 **`cancel`** 时，正在执行的 **`terminal`** 子进程会被 **终止**（与无输出超时、墙钟上限触发的终止共用同一套子进程清理逻辑；Windows 上对 shell 使用 `taskkill /T /F` 等，见 `crates/pointer-core/src/tools/terminal.rs`）。
- 工具 JSON 结果中会包含 **`cancelled`: true**（以及流式输出末尾的说明），便于区分「自然退出 / 超时」与「用户中断」。
- **仅结束终端命令、不停止本轮对话**：宿主可调用 **`AppState::abort_terminal_command(conversation_id)`**（桌面 Tauri 命令 **`abort_terminal_command`**；网页服务端 **`POST /api/chat/:conversation_id/abort-terminal`**，响应体 **`{ "aborted": boolean }`**）。此时子进程同样被清理，但 **`CancellationToken` 不触发**；工具结果里为 **`runAborted`: true**（与 **`cancelled`** 区分）。同一会话在 **`terminal` 执行中** 时，前端工具卡可提供「结束命令」按钮调用该路径。

## 工具：`task_board`

- 注册名：`task_board`；行为通过 **`task_board:replace`** / **`task_board:patch`**（与 qualified `tool_name` 解析一致）。
- 存储：`AppState` 上的 **`TaskBoardStore`**（内存，按 **存储键** 分区）。
- **主会话（单智能体 / Supervisor 主消息）**：存储键为聊天 **`conversation_id`**；`task_board` 的 **`_conversation_id`** 使用该键。每轮 **`[TASK_BOARD]`** 快照由 **`TaskBoardSnapshotHook`** 写入 system **dynamic** 分区（合并顺序在 cacheable 的 Environment / JSON tail **之后**）。见 **[`llm-prompt-assembly-order.md`](llm-prompt-assembly-order.md)**。
- **Supervisor 子 Agent**：与主会话 **隔离**。存储键为  
  **`{conversation_id}\x1fptr_sub_agent\x1f{supervisor_task_id}`**（实现见 `sub_agent_task_board_store_key`）。  
  子 Agent 的 **`[TASK_BOARD]`** 快照同样经 **`before_main_llm_call`** 注入（每轮在 **`generate_tools_system_appendix`** 产出追加之后）；**`task_board`** 读写只针对该子任务键，**不会**看到或修改主会话任务板。
- **可信会话键**：宿主在 `invoke` 前写入 **`_conversation_id`**，覆盖模型可能传入的同名字段，防止伪造；子 Agent 路径下写入的是上述 **子任务键**，不是裸 `conversation_id`。
- 侧车标记：注册为 **`ToolEntry::new_sidecar`**（宿主侧 **`validate_envelope_tool_batch`** 等约束）；用法与 **`response` / `<sidecar_tools>`** 约定见 **`COMMUNICATION_PUBLIC`** 及各工具 **`doc_markdown`**（经 **`generate_tools_system_appendix`** 进入系统提示中的 **`## Tools`**）。未授权该工具时不会出现在上述附录中。

## XML：`<sidecar_tools>` + 根级主工具

- 在 **`<response>`** 内，**可选** **`<sidecar_tools>`**，其中零或多个 **`<call>`**，每个子结构与单工具相同（**`tool_name` / `tool_args`**）。
- **根级仍恰好一对** **`tool_name` / `tool_args`**，表示本回合 **主工具**。
- **执行顺序**：先顺序执行所有侧车 **`call`**，再执行根级主工具（与产品约定一致）。
- **宿主校验**（`validate_envelope_tool_batch`）：若一轮解析出 **多条** `ToolCall`，则除最后一条外必须均为 **侧车工具**；最后一条 **不得** 为仅侧车工具。非法组合不执行本批，并注入环境反馈消息让模型重试（占用工具轮次预算，与空工具 XML 重试类似）。

## 系统提示：工具文档与侧车约定

- 各工具的详细说明来自其 **`doc_markdown`**（通常 `include_str!("prompts/…")`），与授权列表一起在 **`generate_tools_system_appendix`** 中拼入系统提示（**`## Tools`** 等）。
- **`COMMUNICATION_PUBLIC`**（英文）：**`thoughts`** 摘要语义、**`response`** 用法、**`task_board`** 与 **`<sidecar_tools>`** 的通用约定。多步计划的 **字段与侧车规则** 以 PUBLIC 为准；**桌面** **`tool_args.wait`** 见 **`computer/COMMUNICATION.md`**（**Post-action `wait` in `tool_args`**）；**Coder** 专属的 **Definition of done** 与 **Cross-surface verification** 见 **`coder/COMMUNICATION.md`**。

## Agent 白名单

- 在对应 **`AGENT.md`** 的 **`accessPolicy.allowTools`**（及 computer 的 **`toolNames`** 若使用）中加入 **`task_board`**，模型才会在提示中看到该工具并合法调用。
- Supervisor 主流程本身不跑子 Agent 工具循环；子 Agent 各自按上表授权。
- 子 Agent 的 **`task_board`** 与主会话 **分区存储**（见上文「Supervisor 子 Agent」）；若需要把主会话进度写进子任务，由 Supervisor 在 **`instruction`** 文本中自行摘要，而不是共享存储键。

## 任务粒度与 `verification` 字段

- 板上一行应对应 **可独立验收** 的里程碑；**`verification`** 用一句话写清「拿什么证据算过」（一次命令、一次关键读文件、或明确桌面结果）。
- **`done`** 仅在有证据或已写 **`risk note`** 后更新；禁止「改完即 done」式敷衍。

## 与压缩上下文的关系

- 每轮注入 **`[TASK_BOARD]`** 可降低任务板只存在于旧 tool 消息里被压掉的风险。
- 若后续在 **`context_compression`** 中增加高保留信号，可将 **`TASK_BOARD` / `task_board`** 输出纳入优先级（可选增强）。

## 相关代码入口（维护索引）

- 公共提示词：`crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md`
- 工具 XML 说明：`crates/pointer-core/src/tools/prompts/response.md`
- 侧车解析与多 `ToolCall`：`crates/pointer-core/src/tool_envelope.rs`、`crates/pointer-core/src/json_tool_caller.rs`、`crates/pointer-core/src/provider.rs`
- 批校验与工具注册：`crates/pointer-core/src/tools/mod.rs`
- 会话注入与执行：`crates/pointer-core/src/chat_service/`（主流程 `session_inner.rs`，单智能体 `single_agent.rs` + 薄封装，子 Agent `sub_agent.rs` + `sub_agent_prompt.rs` / `sub_agent_stream.rs`，共用 `agent_stream_round.rs` / `agent_post_stream.rs` / `agent_tool_pass.rs`）；任务板快照钩子：`crates/pointer-core/src/extensions/task_board_hook.rs`
