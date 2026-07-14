# 助手消息 UI：`thoughts` 与 API `reasoning`

本文约定 **主气泡展示**、**竖线进度**、**原始输出调试面板** 的分工，避免后续改动把 `reasoning` 当成主界面正文或把竖线改成条形进度条/「仅可见通道」而偏离产品意图。

## 两个通道（勿混用）

| 字段 | 来源 | 主气泡（`AssistantModelMessage` / `AgentMessageBody`） |
|------|------|-------------------------------------|
| **`thoughts`** | 正文 JSON 对象中的 `thoughts` 字符串（解析后写入 `message.thoughts` 或子 session） | **当前 LLM 回合流式输出时**：固定约 **2 行**高度局部预览；**该回合 `message_end` 后默认隐藏**。调试模式且勾选「显示 thoughts 摘要」时：不限高度，回合结束后仍保留。 |
| **`reasoning`** | 兼容 OpenAI 的 **`reasoning_content`** 增量 | **不展示正文**：不得拼进主区 Markdown；仅用于持久化/API 与 `RawWirePanel` 调试。 |

## 「原始输出」（代码图标）

- 入口：助手消息完成后，复制按钮旁的 **代码图标**（受设置 `rawContentViewEnabled` 控制）。
- 面板：`RawWirePanel`，合并展示 **推理（若有）** 与 **正文通道**（优先 `rawContent`，否则回退 `content`）。
- 正文通道**即使与主气泡相同也要显示**（勿因 `rawContent === content` 隐藏），否则无 `MEDIA:` 剥离时面板会只剩推理、看起来像“没有输出”。
- **`reasoning` 只应出现在此面板内**；子 Agent 边框展开后标题栏右侧也有独立 **代码图标**（同一设置开关）。

## 无 `headline` 时的竖线进度（`|`）

- 实现：`AgentMessageBody.vue` 中 **`streamedCharCount`**。
- **必须**取 `content`、`thoughts`、`toolNamePreview`、`responseTextDraft`、`reasoning` 等长度的 **最大值**。
- **UI 为逐段增加的 `|` 字符，不是条形进度条**。

## 后端与前端事件

- 主 Agent 流式：`reasoning_delta`、`assistant_json_partial`、`message_end` 等 **无 `traceId`** → 写入父消息根字段。
- 子 Agent 流式：`reasoning_delta` / `raw_content_delta` 等带 **`traceId`** → 写入 `trace.session`（含 `reasoning`、`rawContent`），**不污染**父消息根字段。
- 子 Agent **`response` handoff** 仍进入 tool result 供父 Agent 推理，但 UI **不展示**（`SubAgentFrame` + `hideResponse`）。
- 子 Agent 每轮 LLM 结束发带 `traceId` 的 `message_end`，`session.contentStreaming=false`，thoughts 收起。

## 子 Agent 边框（`SubAgentFrame`）

- 数据：`agent_step`（`depth > 0`）+ 带 `traceId` 的流式事件 → `AgentTrace.session`。
- 布局：与主 Agent 同构（`AgentMessageBody`：thoughts / headline / 竖线 / 工具卡），包在 **大边框** 内。
- **完成后自动收缩**为一行概要（含任务板在内的全部内容一并隐藏）；点击可展开。`completed` / `failed` 且无 session 时 UI 仍默认收缩。
- 收缩摘要：**统一按工具分桶计数**，再按子 agent `agentId` 展示不同维度——`explore`：搜索/读文件；`coder`：搜索/读文件/终端/编辑；`general-worker`：终端/技能/媒体/搜索/读文件/编辑；`computer`：鼠标/输入/其他；`research`：联网搜索。
- 子 Agent **任务板**与外层相同组件 `TaskBoardPanel`，绑定在 **lead assistant 消息**（`task_board_updated.anchorMessageId` → `childBindings`），展示在对应 `SubAgentFrame` 内。
- 设置「显示子 Agent 边框面板」（`showSubAgentTrace`）：Supervisor 默认开；worker lead 默认关。
- Supervisor 规划列表：`supervisor_plan` → `message.supervisorPlanTasks`，轻量 checklist（无 `<pre>` 时间线）。

修改 `AssistantModelMessage.vue`、`AgentMessageBody.vue`、`SubAgentFrame.vue`、`ModelThoughtPanels.vue` 或 `chat.ts` 中 `traceId` 路由前，请先对照本文。
