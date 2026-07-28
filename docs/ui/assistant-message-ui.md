# 助手消息 UI：`thoughts` 与 API `reasoning`

本文约定 **主气泡展示**、**竖线进度**、**原始输出调试面板** 的分工，避免后续改动把 `reasoning` 当成主界面正文或把竖线改成条形进度条/「仅可见通道」而偏离产品意图。

## 两个通道（勿混用）

| 字段 | 来源 | 主气泡（`AssistantModelMessage` / `AgentMessageBody`） |
|------|------|-------------------------------------|
| **`thoughts`** | 正文 JSON 对象中的 `thoughts` 字符串（解析后写入 `message.thoughts` 或子 session） | **当前 LLM 回合流式输出时**：固定约 **2 行**高度局部预览；**该回合 `message_end` 后默认隐藏**。调试模式且勾选「显示 thoughts 摘要」时：不限高度，回合结束后仍保留。 |
| **`reasoning`** | 兼容 OpenAI 的 **`reasoning_content`** 增量（亦接受 vLLM/部分 Qwen 的 **`reasoning`** 字段名） | 设置开启「显示推理过程」时展示；否则仅用于持久化/API 与 `RawWirePanel` 调试。不得拼进主区 Markdown。 |

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
- 子 Agent 每轮 LLM 结束发带 `traceId`（及可选 `scopedMessageId`）的 `message_end`，`session.contentStreaming=false`，thoughts 收起。
- **主回合结束只认 `StreamEvent::Done`**（及 `error` / 用户停止）。前端不再用 lead `message_end` 后的启发式兜底清 `generating`（旧 `scheduleMaybeFinishGenerating` 会在工具轮间隙 / 子 Agent 结束后误清状态并提前冲出站队列）。
- **`error` / `done`**：必须带 **`conversationId`**。前端按该 id 落消息 / 清 run state，**禁止**默认写到当前打开会话（后台会话失败时否则会串会话）。`error` 有 `messageId` 时优先按消息定位，并以事件里的 `conversationId` 作为 `findMessage` 偏好会话。

## 子 Agent 边框（`SubAgentFrame`）

- 数据：`agent_step`（`depth > 0`）+ 带 `traceId` 的流式事件 → `AgentTrace.session`。
- 布局：与主 Agent 同构（`AgentMessageBody`：thoughts / headline / 竖线 / 工具卡），包在 **大边框** 内。
- **边框色**：默认 `border-border` + `bg-card`；`failed` 时用 `border-danger/35` + `bg-danger/5`（主题语义色，随浅/深翻转）。展开箭头用 `text-muted`，**不用** accent 紫边/紫箭头。
- **嵌套位置**：`AgentTrace.parentToolCallId` 指向父消息里对应的 **`run_subagent`** 工具行；UI 将子 Agent 边框（及子任务板）紧跟在该工具行下方。无该字段或找不到工具行时，回退到消息底部（兼容旧会话）。并行多个 explore / self 时各自挂到自己的委派行下。owned-wave（explore / self）在拿到并发许可后立刻发 `status=running` 的 `agent_step`（已带 `parentToolCallId`），避免只在结束时才关联。
- **默认收缩**为一行概要（含任务板在内的全部内容一并隐藏），执行中与完成后均如此；点击可展开。流式 `agent_step` 不得覆盖用户手动展开状态。收缩态**不**渲染工具卡片。
- 收缩摘要：`running` 且已有工具调用时，优先展示**当前/最近工具**一行（与紧凑状态条同款文案）；结束后再按工具分桶计数，维度按子 agent `agentId`——`explore`：搜索/读文件；`coder`：搜索/读文件/终端/编辑；`computer`：鼠标/输入/其他；`research`：联网搜索。历史 trace 中的 `general-worker` 仍按既有 metadata 渲染（`agentUi` / `subAgentStats`），registry 不再加载该 agent。
- 子 Agent **任务板**与外层相同组件 `TaskBoardPanel`，绑定在 **lead assistant 消息**（`task_board_updated.anchorMessageId` → `childBindings`），展示在对应 `SubAgentFrame` 内。
- 设置「显示子 Agent 边框面板」（`showSubAgentTrace`）：Supervisor 默认开；worker lead 默认关。
- Supervisor 规划列表：`supervisor_plan` → `message.supervisorPlanTasks`，轻量 checklist（无 `<pre>` 时间线）。

## `ask_user` 工具行

- 工具行标题只显示 **「询问用户」**（不加 `displaySummary` / 问题摘要）。
- 问题正文只出现在交互卡片（`AskUserOptions`）内，避免标题与正文重复。
- 宿主固定提供「其他」自由输入（对齐 Hermes）；不必要求模型在 `options` 里加 Other。
- 选中态只用主题语义色：`foreground` / `background` / `border` / `hover` / `muted`（随浅色/深色翻转）。
  勾选填充为 `bg-foreground/55 text-background`，不要写死灰阶、紫色或未定义的 `muted-foreground`。

修改 `AssistantModelMessage.vue`、`AgentMessageBody.vue`、`SubAgentFrame.vue`、`ModelThoughtPanels.vue` 或 `chat.ts` 中 `traceId` 路由前，请先对照本文。
