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
- 工具槽第二行用 `thinkingLabel` 静态点数（与工具间隙相同）。不要再画一块独立的「思考中」行。
- 一旦出现用户可见的正文、回复草稿或工具行，整行「思考中」立即收起（含触顶后的三点闪烁）。隐藏的 `thoughts` / `reasoning` 只用来加点数，不单独把这一行留下来。
- **回合开头、还没有任何工具时**：把「思考中.」画在工具槽里的一行摘要上（不要空的第一行再滑入第二行）。第一个工具出现时画完整工具行。
- **已有过程工具时**：不要另起一块「思考中」（会按正文间距拉开）。把它放在 `CollapsedRunHeader` 第二行；当前工具仍占第二行，工具间隙第二行换成「思考中.」，下一个工具从下方把它顶上去。**正文（content / 回复草稿）一旦出现，第二行思考立刻收掉**，不要停在工具摘要上。

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
- **单行汇总**：有子任务框时，收起态**不显示**宿主「委派子任务」工具行（等确认 / 等终端输入除外）。一行里写任务目标 + 内层工具统计，末尾可带耗时。左侧固定细线分叉图标（与工具行箭头同一套线型），用来和普通工具摘要区分（普通组没有前置图标）。
- **当前任务**：运行时固定两行，工具收缩与子任务共用 `CollapsedRunHeader`。第一行是汇总。第二行：正在跑的工具，或工具间隙的「思考中.」。当前工具结束、思考开始时从下方顶上来；下一个工具出现时同样把「思考中」顶走。第二行只写工具名和摘要，不跟「执行中」。子任务第二行相对图标的缩进写在裁切层 `left` 上。系统「减少动态效果」时只换文案。整段结束后第二行才消失。多轮工具之间的空 streaming 壳并进当前工具段，不要当成新的助手正文（否则会误加 `mt-7`）。
- 布局：与主 Agent 同构（`AgentMessageBody`：thoughts / headline / 竖线 / 工具卡）。收缩态**不要**再套一层卡片（无 `rounded-xl` / 额外 `p-3` / `px-1`），与连续工具摘要同一左缘。
- **收缩交互**：与连续工具组相同——`13px` `text-muted` 单行摘要，箭头在文案后；收起时桌面悬停 / 键盘聚焦才显现（触控端始终显示）；**展开后箭头固定显示**。失败用 `text-danger`。展开后标题仍用同一行摘要（不要改成英文 `running` / `completed`）。
- **嵌套位置**：`AgentTrace.parentToolCallId` 指向父消息里对应的 **`run_subagent`** 工具行；UI 将子 Agent 摘要（及子任务板）挂在该工具位置（收起时替换工具行）。无该字段或找不到工具行时，回退到消息底部（兼容旧会话，补 `px-3` 对齐）。并行多个 explore / self 时各自一条汇总。owned-wave（explore / self）在拿到并发许可后立刻发 `status=running` 的 `agent_step`（已带 `parentToolCallId`），避免只在结束时才关联。
- **默认收缩**为一行概要（工具卡片不渲染；任务板仍显示在摘要上方），执行中与完成后均如此；点击可展开。流式 `agent_step` 不得覆盖用户手动展开状态。收缩态**不**渲染工具卡片。展开后框内连续过程工具同样走 `ToolCallGroup` 收缩。
- 收缩摘要：目标文案取宿主工具的 `displaySummary`（`title` / `goal`）；没有则回退角色名。结束后按工具分桶计数（完成态不写「已完成」），维度按子 agent `agentId`——`explore`：搜索/读文件；`coder`：搜索/读文件/终端/编辑；`computer`：鼠标/输入/其他；`research`：联网搜索。失败才带「失败」。历史 trace 中的 `general-worker` 仍按既有 metadata 渲染（`agentUi` / `subAgentStats`），registry 不再加载该 agent。
- 子 Agent **任务板**与外层相同组件 `TaskBoardPanel`，绑定在 **lead assistant 消息**（`task_board_updated.anchorMessageId` → `childBindings`），渲染在对应 `SubAgentFrame` **内、执行过程上方**；收缩与展开时都显示完整任务板，不随工具区折叠隐藏。
- **子板统一查找**：先按绑定（trace id / 旧 lead 消息 id）命中；没有绑定再按同一 task id 找未绑定板（self-fork 时优先匹配 trace 里的 instance）。legacy 短 key 与带 `ptr_agent_instance` 的长 key 走同一套规则。
- 设置「显示子 Agent 边框面板」（`showSubAgentTrace`）：Supervisor 默认开；worker lead 默认关。
- Supervisor 规划列表：`supervisor_plan` → `message.supervisorPlanTasks`，轻量 checklist（无 `<pre>` 时间线）。

## 工具行耗时

- 展示为整数秒（`1s`、`2s`），不足 **1s** 不显示。
- 数据仍存 `durationMs`，只改 UI 文案。

## 连续工具调用收缩

主 Agent 过程区里，**连续已完成**的过程工具（读/搜/列目录/终端/技能等）默认收成一行摘要，交互对齐子 Agent 收缩态：`13px` 单行文案，箭头在文案后。收起时桌面悬停 / 键盘聚焦才显现（触控端始终显示）；**展开后箭头固定显示**。点击展开为原来的工具行。

三层不要混成一块：

| 层 | 样式 |
|----|------|
| **工作** | `11–12px`、`text-muted/70`（回合耗时）。下一条正文间距 `mt-1.5`，与工具行紧贴用户气泡时相同 |
| **工具** | `13px`、`text-muted`（收缩摘要；悬停略加深）；与「工作」/ 正文同一 `px-3` 左缘，摘要按钮不要再加 `px-1` |
| **正文** | `15px`、`text-foreground`（助手回复）。工具块后下一条正文不再加 `mt-3.5`：上下各 `0.75rem` 由 `.message-stamp-host` 承担，避免和时间戳槽叠出「上窄下宽」。 |

时间戳 / 复制只挂在**助手正文和用户气泡**下面，不要跟在工具摘要后面。正文容器上下各留 `0.75rem`（`.message-stamp-host`），时间戳落在下边距里。正文后的工具块、以及工具后再跟的正文都不要再加 `mt-2` / `mt-3.5`，否则静态时会下边比上边宽。悬停才显现。
用户气泡（`.message-user-bubble`）四周 `1rem`；时间戳落在下边距里（距底 `0.25rem`），上下留白对称。

- 摘要文案 Cursor 风格，例如「探索 14 个文件，3 次搜索，执行 5 条命令」。
- **不限于同一轮 LLM**：只要过程工具在界面上连续（中间没有用户可见正文、询问、子 Agent、任务板），就收进同一行。多轮工具-only 回合会拼成一份列表再分组。
- 执行中的工具挂在同一 `CollapsedRunHeader` 第二行（与子任务相同）；工具间隙第二行改为「思考中.」，下一个工具开始时从下方顶上来。**只有 1 条进行中的工具时画完整工具行**，不要空摘要行，也不要从「思考中」再滑入一行。有已完成汇总、后面还在跑时，再用两行头。全部结束后仍是仅 1 条不收缩。
- **打断合并**：用户可见助手正文、`ask_user`、`run_subagent`、等待确认 / 等待终端输入、生图/视频、任务板、环境/重试 glue。**媒体理解**与读文件一样并进摘要。
- 会话搜索命中组内工具时自动展开。

## 工具行路径

文件类工具（读/写/编辑等）摘要用工作区相对路径，**不要**从左侧截成「目录…」。
行宽不够时与轮次修改摘要相同：`.ellipsis-start`（`direction: rtl` + `&lrm;`），省略号在左，文件名在右；`title` 仍是完整相对路径。
无 CSS 宽度的一行状态（紧凑坞、子任务收缩摘要）用 `truncatePathKeepEnd`，同样保尾。

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
