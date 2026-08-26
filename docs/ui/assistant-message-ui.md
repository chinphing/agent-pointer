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

## 「思考中…」点数（`ThinkingIndicator`）

流式尚未露出可见正文/工具时，标题为「思考中」加逐段增加的 `.`。

- 字符数取 `content` / `rawContent` / `thoughts` / `reasoning` 等长度的 **最大值**。
- **阶梯**：前 10 个点每个 **100** 字符；之后每满 10 个点，单点覆盖字符数 **×2**（200 / 400 / 800 / 1600）。
- 最多 **48** 个点，合计约 **27800** 字符后不再增加。尚无流出时也显示 1 个点。
- 点数触顶后末尾 3 个点继续依次明暗，表示仍在执行（尊重系统「减少动态效果」）。
- 一旦出现用户可见的正文、回复草稿或工具行，整行「思考中」立即收起（含触顶后的三点闪烁）。隐藏的 `thoughts` / `reasoning` 只用来加点数，不单独把这一行留下来。

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
- **默认收缩**为一行概要（工具卡片不渲染；任务板仍显示在摘要上方），执行中与完成后均如此；点击可展开。流式 `agent_step` 不得覆盖用户手动展开状态。收缩态**不**渲染工具卡片。
- 收缩摘要：`running` 且已有工具调用时，优先展示**当前/最近工具**一行（与紧凑状态条同款文案）；结束后再按工具分桶计数，维度按子 agent `agentId`——`explore`：搜索/读文件；`coder`：搜索/读文件/终端/编辑；`computer`：鼠标/输入/其他；`research`：联网搜索。历史 trace 中的 `general-worker` 仍按既有 metadata 渲染（`agentUi` / `subAgentStats`），registry 不再加载该 agent。
- 子 Agent **任务板**与外层相同组件 `TaskBoardPanel`，绑定在 **lead assistant 消息**（`task_board_updated.anchorMessageId` → `childBindings`），渲染在对应 `SubAgentFrame` **内、执行过程上方**；收缩与展开时都显示完整任务板，不随工具区折叠隐藏。
- **子板统一查找**：先按绑定（trace id / 旧 lead 消息 id）命中；没有绑定再按同一 task id 找未绑定板（self-fork 时优先匹配 trace 里的 instance）。legacy 短 key 与带 `ptr_agent_instance` 的长 key 走同一套规则。
- 设置「显示子 Agent 边框面板」（`showSubAgentTrace`）：Supervisor 默认开；worker lead 默认关。
- Supervisor 规划列表：`supervisor_plan` → `message.supervisorPlanTasks`，轻量 checklist（无 `<pre>` 时间线）。

## 工具行耗时

- 展示为整数秒（`1s`、`2s`），不足 **1s** 不显示。
- 数据仍存 `durationMs`，只改 UI 文案。

## `ask_user` 工具行

- 工具行标题只显示 **「询问用户」**（不加 `displaySummary` / 问题摘要）。
- **问题**写在围栏 **header**（表格 `th` 同款：`--hover` 底、底部分隔线），选项在 header 下方（`--card`，与表格 `td` 一致）。
- 问题与选项包在 **`.fence-block`** 里：外框与表格相同（`rounded-lg border-border`），不要做成白底扁平列表。
- 卡片与工具行同一左缘（不要 `ml-4`）。问题、选项勾选、「其他」与表头一样走 **`px-3` 左内边距**，选项不要 `-mx-1` 把左缘拉偏。
- 宿主固定提供「其他」自由输入（对齐 Hermes）；不必要求模型在 `options` 里加 Other。
- 选项文案（含 description）在卡片宽度内**自动换行**，不要用 `truncate` 单行截断。
- 「其他」输入框设 `max-w-[14rem]`，不要 `flex-1` 拉满整行。
- 选中态只用主题语义色：`foreground` / `background` / `border` / `hover` / `muted`（随浅色/深色翻转）。
  勾选填充为 `bg-foreground/55 text-background`，不要写死灰阶、紫色或未定义的 `muted-foreground`。

## 上下文压缩进行中标记

- 事件：`context_compression_started` → 会话 run state 的 `contextCompressing`。
- 展示：插在摘要切分点（`insertBeforeMessageId` 之前），工具行样式（`ContextCompressingMarker`），含旋转「压缩中」。切点在过程行上时，收缩态不把标记挪到最终回复前（随「工作」隐藏）。找不到切分消息时回退到当前回合或列表末尾。
- **子 Agent**：切点在子任务隔离历史上，标记画在对应 `SubAgentFrame` 过程区同一位置，不作为与「委派子任务」同级的外层过程行。不自动展开子任务框；收缩时随过程一起隐藏（与主会话切点在过程行上时相同）。切点 id 尚未进 UI 时落在该子任务过程末尾。框内文案与主会话相同（「正在压缩较早记录」）。
- 隐藏：`context_compression_applied` / `context_compressed`，或停止 / `done` / `error` 清 run state。
- **不**写入聊天记录，**不**进入模型上下文。详见 [`../internals/context-compression.md`](../internals/context-compression.md)。
- **超限重试**：`MessageStart` 之后 LLM 400 再同步压缩时，必须先 `message_end` 关掉空壳，再开新的 `MessageStart`。否则旧行一直 `streaming`，界面停在「思考中.」。新的 `message_start` 也会把同会话里其它空的 streaming 壳收掉。

## 空回复 / 环境恢复重试提示（对用户隐藏）

模型偶发返回空内容（无正文、无工具调用）时，后端会注入一条 user 行（如「你的上一次回复为空…（异常重试 1/3）」）并自动重试；限流 / 网关异常 / 输出截断的 `【环境反馈】`、`【输出长度】` 同类。

- **对模型**：仍进入 history（`push_injected_format_retry_turn`），用于纠偏下一轮。
- **对用户**：不展示为聊天气泡；`isInternalRetryUserMessage` / silent glue 处理，也不作为 turn 锚点或会话标题来源。
- 重试耗尽后的真正错误文案仍会正常展示。

修改 `AssistantModelMessage.vue`、`AgentMessageBody.vue`、`SubAgentFrame.vue`、`ModelThoughtPanels.vue` 或 `chat.ts` 中 `traceId` 路由前，请先对照本文。
