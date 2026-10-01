# 助手消息 UI：`thoughts` 与 API `reasoning`

本文约定 **主气泡展示**、**竖线进度**、**原始输出调试面板** 的分工，避免后续改动把 `reasoning` 当成主界面正文或把竖线改成条形进度条/「仅可见通道」而偏离产品意图。

## 两个通道（勿混用）

| 字段 | 来源 | 主气泡（`AssistantModelMessage` / `AgentMessageBody`） |
|------|------|-------------------------------------|
| **`thoughts`** | 正文 JSON 对象中的 `thoughts` 字符串（解析后写入 `message.thoughts` 或子 session） | **当前 LLM 回合流式输出时**：固定约 **2 行**高度局部预览；**该回合 `message_end` 后默认隐藏**，并从界面内存卸掉。调试模式且勾选「显示 thoughts 摘要」时：不限高度，回合结束后仍保留，这时再从数据库取回。 |
| **`reasoning`** | 兼容 OpenAI 的 **`reasoning_content`** 增量（亦接受 vLLM/部分 Qwen 的 **`reasoning`** 字段名） | 设置开启「显示推理过程」时展示；否则仅用于持久化/API 与 `RawWirePanel` 调试。不得拼进主区 Markdown。一轮结束后从界面内存卸掉，打开上述面板时再取回。数据库里的全文仍在，下一轮请求用的是那一份。 |

## 「原始输出」（代码图标）

- 入口：助手消息完成后，复制按钮旁的 **代码图标**（受设置 `rawContentViewEnabled` 控制）。
- 面板：`RawWirePanel`，合并展示 **推理（若有）** 与 **正文通道**（优先 `rawContent`，否则回退 `content`）。
- 正文通道**即使与主气泡相同也要显示**（勿因 `rawContent === content` 隐藏），否则无 `MEDIA:` 剥离时面板会只剩推理、看起来像“没有输出”。
- **`reasoning` 只应出现在此面板内**；子 Agent 边框展开后标题栏右侧也有独立 **代码图标**（同一设置开关）。

## 「思考中…」点数（`ThinkingIndicator`）

流式尚未露出可见正文/工具时，标题为「思考中」加逐段增加的 `.`。

- 点数只跟 **当前这一轮**。`contentStreaming === true` 的行才计数；`message_end` 后仍 `status: streaming` 的旧行不算。尚无本轮流出时固定 1 个点，不要用上一轮 `thoughts` 起跳。
- 本轮字符取 `rawContent` / `thoughts` / `reasoning` 等长度的 **最大值**，**不要数 `content`**：展示合并会把上一轮正文拷进去，点数会被旧回复锁死。子 Agent 不要用 `latestStreamBody`；只数当前 `contentStreaming` 行（以及仍在 streaming 的 session）。
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
- 子 Agent **`response` handoff** 仍进入 tool result 供父 Agent 推理；UI 在对应 `SubAgentFrame` 内按轮次显示该子 Agent 的 `content`（含中间结论与最终 handoff），不再是「不展示」。
- 子 Agent 每轮 LLM 结束发带 `traceId`（及可选 `scopedMessageId`）的 `message_end`，`session.contentStreaming=false`，thoughts 收起。
- **主回合结束只认 `StreamEvent::Done`**（及 `error` / 用户停止）。前端不再用 lead `message_end` 后的启发式兜底清 `generating`（旧 `scheduleMaybeFinishGenerating` 会在工具轮间隙 / 子 Agent 结束后误清状态并提前冲出站队列）。
- **`error` / `done`**：必须带 **`conversationId`**。前端按该 id 落消息 / 清 run state，**禁止**默认写到当前打开会话（后台会话失败时否则会串会话）。`error` 有 `messageId` 时优先按消息定位，并以事件里的 `conversationId` 作为 `findMessage` 偏好会话。

## 子 Agent 边框（`SubAgentFrame`）

- 数据：`agent_step`（`depth > 0`）+ 带 `traceId` 的流式事件 → `AgentTrace.session`。
- **三行**：宿主委派工具行**始终显示**（分叉图标 + 目标，展开看参数，与普通工具行相同；不要再写「委派子任务」）。其下才是过程：第二行内层工具统计，第三行运行中的当前工具 /「思考中.」。不要把目标、次数、当前工具揉进同一行。
- **当前任务**：统计行 + 运行时与主会话 `ToolCallGroup` 相同。分叉图标画在第一行目标上（与工具行箭头同一套线型），统计行**不要**再放图标。第二行只写次数（`搜索 N 次 · 读文件 N 次 · 终端 N 次`），不写任务目标，也**不要**再缀「后台执行中」（后台态只留在宿主工具行）。第三行：正在跑的工具（种类图标 + 摘要，不要再写「终端命令」这类种类名），或工具间隙的「思考中.」（子任务未完成且没有内层工具在跑时也要显示，包括首轮工具前）。当前工具结束、思考开始时从下方顶上来；下一个工具出现时同样把「思考中」顶走。第三行不跟「执行中」。统计行、运行时行和展开后的内层工具相对第一行缩进约图标宽 + 间距，与委派标题文字对齐，不要和父级工具行左缘齐平。结束后收成轻量 stub 也走同一套 `.sub-agent-nested` 缩进，不要和进行中对不齐。系统「减少动态效果」时只换文案。整段结束后第三行才消失。多轮工具之间的空 streaming 壳并进当前工具段，不要当成新的助手正文（否则会误加 `mt-7`）。
- 布局：收缩态**不要**再套一层卡片（无 `rounded-xl` / 额外 `p-3` / `px-1`），与连续工具摘要同一左缘。
- **收缩交互**：统计行与连续工具组相同——`13px` `text-muted` 单行摘要，箭头在文案后；收起时桌面悬停 / 键盘聚焦才显现（触控端始终显示）；**展开后箭头固定显示**。悬停底只包统计文案、箭头，以及第二行「思考中」/当前工具，不要拉满列宽。失败用 `text-danger`。展开后统计行仍用同一行次数（不要改成英文 `running` / `completed`）。点统计行展开为内层一条条工具行，**不要**再套一层「探索 N 个文件」摘要。**统计行默认显示**（收缩的是内层工具，不是次数行）。次数写在 `agentTrace.summaryLine`，hydrate 不拉过程明细；缺摘要时补一次 scoped 算出次数再卸掉。补算前暂显「过程」。**进行中**若已有第三行 live（当前工具 /「思考中」），不要再用「过程」占统计行——与父会话首轮只有思考行一致。后台子 Agent 在父回合已返回 job 句柄后仍以 `trace.status=running` 为准显示工具间隙「思考中」，**不要**因 lead 已不 `generating` 而只剩「过程」。
- **默认收缩与轨迹**：收缩态只看到统计 + 当前工具/思考；完整工具轨迹要点开统计行展开。展开后应列出内层工具行（流式中来自 scoped store；终态 stub 会 lazy load）。**默认收缩**统计行（内层过程工具卡片不渲染；任务板仍显示在摘要上方），执行中与完成后均如此。流式 `agent_step` 不得覆盖用户手动展开状态。
- **帧内正文**：展开帧按轮次交错渲染「该轮 `content` → 该轮工具行」（`buildSubAgentRoundsFromScoped`：每个 assistant scoped 行 = 一轮，不合并）；正文用轻量 `SubAgentContentBlock.vue`（复用 `parseMarkdown` + 流式节流），**不带** lead 专属 chrome（复制按钮 / 媒体 / 任务板 / 平台余额）。折叠帧**不显示正文**（只有统计摘要行，与折叠工具组一致；原「一行正文预览」已按决定移除）；无 scoped 行的旧会话退化为「单轮仅工具」。运行中尚未归属某轮的工具作为尾轮追加。
- **会话搜索覆盖 scoped 行**：`findCurrentConversationMatches(messages, query, scopedRows)` 把 scoped 行（深层工具行与正文）纳入搜索面；命中以该帧**锚点消息**为 `messageId`，并带 `toolCallId` 或 `contentMessageId`。正文命中定位选择器 `[data-sub-agent-content-id]`。命中子孙层时用 `traceSubtreeContainsSearchTarget` **自动展开全部祖先帧**，stub 不被降级。
- **嵌套位置**：`AgentTrace.parentToolCallId` 指向父消息里对应的 **`run_subagent`** 工具行；UI 将统计/过程挂在该工具行**下方**（`after-tool`）。无该字段或找不到工具行时，回退到消息底部（兼容旧会话，补 `px-3` 对齐，统计行自带目标文案）。并行多个 explore / self 时各自一条。owned-wave（explore / self）在拿到并发许可后立刻发 `status=running` 的 `agent_step`（已带 `parentToolCallId`），避免只在结束时才关联。**每个 fork 结束时立刻发 completed/failed/cancelled**（并清掉宿主行「执行中」），不要等整波 join 才一起改状态。
- **嵌套树**：深层子 Agent 的 trace 也写在**它自己那一层的 scoped 行**上（DB 里有 `agentTrace.parentTraceId`）。渲染用 `buildSubAgentTraceTree`（`src/lib/subAgentTraceTree.ts`）按 `parentTraceId` 建树，`SubAgentFrame` 在过程块之后递归渲染 `childrenOf(trace.id)`；缺 `parentTraceId` 的旧行回退挂到宿主工具行。折叠父帧 → 子帧一并隐藏；展开父帧 → 子帧按各自状态渲染。`parentTraceId` 成环/悬空时按无父处理，不能丢帧。
- **fork 标识**：`AgentTrace.delegation`（`self` / `registered`）由后端埋点。`self` fork 在标签后加 **` (fork)`**（`SELF_FORK_LABEL_SUFFIX`，如 `coder (fork)`），与真 worker 区分；`self` fork 的 trace id 含唯一 instance 段，并行 fork 互不覆盖。
- **收缩态仍露出交互卡片**：只有 **`pending_approval`**（与主会话收缩回合的 `isPendingApprovalToolCall` 一致）。`ask_user` 不再靠帧/宿主行保留来外显，改由对话区顶部固定条承载（见下）；摘要第二行的 live 当前工具不重复画这些交互项（卡片本身已是交互面）。
- **父回合「默认收缩执行过程」**：`contentOnly` **不得**关掉仍在跑的 `SubAgentFrame`，否则进行中的子任务过程与嵌套层级会被整段藏掉（`ask_user` 已改由顶部条承载，不再依赖帧保留）。已结束的子任务过程仍只在展开后显示。
- 统计：结束后按工具分桶计数（完成态不写「已完成」，**运行中也不写「进行中」「执行中」**）。失败写在统计数字**后面**（`读文件 1 次 · 失败`）。维度按子 agent `agentId`——`explore`：搜索/读文件；`coder`：搜索/读文件/终端/编辑；`computer`：鼠标/输入/其他；`research`：联网搜索。历史 trace 中的 `general-worker` 仍按既有 metadata 渲染（`agentUi` / `subAgentStats`），registry 不再加载该 agent。
- 子 Agent **任务板**与外层相同组件 `TaskBoardPanel`，绑定在 **lead assistant 消息**（`task_board_updated.anchorMessageId` → `childBindings`），渲染在对应 `SubAgentFrame` **内、执行过程上方**；收缩与展开时都显示（不随工具区折叠隐藏）。样式与工具摘要同一套：默认一行 `13px` muted（**步骤勾选图标** + 目标 + 进度），点开才是步骤列表；不要卡片、不要「active」徽章。摘要只跟执行状态：进行中写「执行中」，完成不写状态，失败写「失败」。**嵌套看板的「执行中」跟子 Agent 是否还在跑**（trace 已结束就不要因看板 `meta.running` 残留继续显示）。不要用空方框清单图标——那是 sidecar「任务板 · 初始化」工具行（默认隐藏；内层过程工具同样走 `visibleToolCalls`，只有设置里打开「显示 sidecar 工具调用」才出现）。嵌套看板与统计行 / 内层工具同一条左缘：缩进画在 `.sub-agent-nested` 内层（过 fork 图标对齐委派标题），不要和 `overflow-hidden` 外框叠在同一层。父会话板靠右（`justify-end` + `w-fit`）时，箭头始终占位、步骤宽度跟摘要走，避免悬停/展开把整块撑开左右跳。
- 嵌套看板、统计行、展开后的内层工具共用 `space-y-0.5`，与连续工具行、统计行到「思考中」相同；看板改成摘要行后不要再按旧卡片留 `space-y-2`。
- **子板统一查找**：先按绑定（trace id / 旧 lead 消息 id）命中；没有绑定再按同一 task id 找未绑定板（self-fork 时优先匹配 trace 里的 instance）。legacy 短 key 与带 `ptr_agent_instance` 的长 key 走同一套规则。

## 工具行耗时

- 展示为整数秒（`1s`、`2s`），不足 **1s** 不显示。
- 数据仍存 `durationMs`，只改 UI 文案。
- 后台 `run_subagent` 宿主行与前台相同：从后台任务启动计到结束，写入 `durationMs`（不要在终态时丢空）。
- **执行中**：只有当前工具的**文字**扫过高光，背景不要亮带。委派宿主行、统计行、「思考中」、图标和「执行中」都不扫。系统「减少动态效果」时关掉。

## 工具行图标

过程工具行用 **Lucide 线框种类图标 + 摘要**（命令 / 路径 / 查询），不要再写「终端命令」「读取文件」这类种类名。图标与箭头同一套描边，一律 `text-muted/70`、14px，不按种类上色。箭头紧跟文案/状态之后（触发按钮内），不要用 `ml-auto` 拉到对话列最右侧。

种类中文名由后端 `tools/display.rs` 写入 `displayLabel`（start 时就可以显示）。摘要优先 `displaySummary`，缺了再读参数里的 **`label`**（终端），媒体理解再读 `goal`。start 只有种类名、参数还没到时不要藏种类名。历史行缺字段时前端 `resolveToolDisplayForCall` 兜底。新增用户可见工具必须两边都补中文，不要把工具 slug（如 `session_search`）直接露在工具行上。`session_search` →「搜索会话」（历史检索图标），`session_read` →「读取会话」（对话文本图标），不要复用文件搜索/读文件图标。`job`（等待/查看后台任务）用沙漏图标，并保留种类名（没有摘要）。

- **仍显示名称**：「询问用户」（没有摘要）、对不上的工具（扳手 + 名称）。委派与其它过程工具一样：图标 + 摘要（目标）。
- **扫光**打在进行中的摘要上（收缩组当前任务行，以及单独的工具行，含终端命令）。斜向窗口露出更亮的字形；进行中一律单行省略，避免换行和亮字对不齐。时长按可见文字宽度算，最长 6 秒。不要打在种类名和图标上。不要用会重绘文字的 `background-clip` 动画。
- **不要**改收缩组的次数行（「探索 19 个文件」）和子任务统计行（「搜索 72 次」）——那些不是单条工具。
- 读屏 / 搜索仍用完整「种类 · 摘要」（`aria-label` / `compactToolCallStatusLine`）。紧凑坞标题也仍用带种类名的字符串。
- 映射在 `src/lib/toolCallKindIcon.ts`。

## 连续工具调用收缩

主 Agent 过程区里，**连续已完成**的过程工具（读/搜/列目录/终端/技能等）默认收成一行摘要，交互对齐子 Agent 收缩态：`13px` 单行文案，箭头在文案后。收起时桌面悬停 / 键盘聚焦才显现（触控端始终显示）；**展开后箭头固定显示**。点击展开为原来的工具行。

三层不要混成一块：

| 层 | 样式 |
|----|------|
| **工作** | `11–12px`、`text-muted/70`（回合耗时）。下一条正文间距 `mt-1.5`，与工具行紧贴用户气泡时相同 |
| **工具** | `13px`、`text-muted`（收缩摘要；悬停略加深）；与「工作」/ 正文同一 `px-3` 左缘，摘要按钮不要再加 `px-1`。悬停底只包文字和箭头（含「思考中」和第二行当前工具），不要拉满列宽 |
| **正文** | `15px`、`text-foreground`（助手回复）。工具块后下一条正文不再加 `mt-3.5`：上下各 `0.75rem` 由 `.message-stamp-host` 承担，避免和时间戳槽叠出「上窄下宽」。 |

时间戳 / 复制只挂在**助手正文和用户气泡**下面，不要跟在工具摘要后面。正文和后续工具在同一条消息里上下相连，但 hover 只包 `.message-stamp-host`，不要包工具行（否则悬停「探索 N 次搜索」也会亮时间戳）。助手正文上下各留 `0.75rem`，时间戳落在下边距里。正文后的工具块、以及工具后再跟的正文都不要再加 `mt-2` / `mt-3.5`，否则静态时会下边比上边宽。悬停才显现。
用户气泡（`.message-user-bubble`）只包正文，内边距 `0.5rem 0.75rem`，**不要**最小宽度、也不要在气泡里给时间戳留空。时间戳 / 复制在气泡外右对齐（可比气泡更宽，往左伸）；悬停气泡所在行才显现。

宿主注入、全文要进模型但气泡只要短句时：写在 **`uiBindings.bubbleText`**（可选 **`hostKind`**），UI 用 `userMessageDisplayContent` 优先读该字段。空闲 job push：`hostKind=idle_job_push`，`content` 第一句与气泡相同并带任务名（不要只写「后台任务已完成。」），终稿接在后面。对话导航预览读 `bubbleText`。

- 摘要文案 Cursor 风格，例如「探索 14 个文件，3 次搜索，执行 5 条命令」。**第一行始终是次数统计**，即使组内还有进行中的工具，也不要用单条工具标题（「联网搜索 · 某查询 · 执行中」）顶替摘要。
- **不限于同一轮 LLM**：只要过程工具在界面上连续（中间没有用户可见正文、询问、子 Agent、任务板），就收进同一行。多轮工具-only 回合会拼成一份列表再分组。
- 执行中的工具挂在同一 `CollapsedRunHeader` 第二行（与子任务相同）；工具间隙第二行改为「思考中.」，下一个工具开始时从下方顶上来。并行工具完成乱序时，只要组内仍有 `running`/`pending`，第二行仍应指向其中一条进行中的工具（不要只认列表末尾）。**只有 1 条进行中的工具时画完整工具行**，不要空摘要行，也不要从「思考中」再滑入一行。有已完成汇总、后面还在跑时，再用两行头。全部结束后仍是仅 1 条不收缩。
- **打断合并**：用户可见助手正文、`ask_user`、`run_subagent`、等待确认 / 等待终端输入、生图/视频、任务板、环境/重试 glue。**媒体理解**与读文件一样并进摘要。
- 会话搜索命中组内工具时自动展开。

## 工具行路径

文件类工具（读/写/编辑等）摘要用工作区相对路径，**不要**从左侧截成「目录…」。
行宽不够时与轮次修改摘要相同：`.ellipsis-start`（外层 `direction: rtl` 省略号在左）+ 内层 `.ellipsis-start-content`（`direction: ltr; unicode-bidi: isolate`）保持路径字形顺序，避免绝对路径开头的 `/` 被画到末尾成「`.py/`」；`title` 仍是完整相对路径。不要只靠文末 `&lrm;`。
无 CSS 宽度的一行状态（紧凑坞、子任务收缩摘要）用 `truncatePathKeepEnd`，同样保尾。

## 询问用户顶部条（`AskUserBanner`）

深层子 Agent 的 `ask_user` 不再靠「保留帧 / 宿主行」外显，改由**对话区顶部固定条**承载：

- 挂载：`ChatView.vue` 消息区容器内、`<MessageList>` 之上（消息区自己滚动，条始终钉在对话区顶部；移动端同位置）。
- 数据源**与帧是否挂载无关**：当前会话 lead `messages[].toolCalls` 里的 `ask_user`（pending / running）＋ `useConversationScopedStore().listRows(convId)` 里的深层 scoped 行。响应式依赖 = `getMembershipSignal` + `getLiveSignal`。
- 队列（`src/lib/askUserBanner.ts`）：显示最早一条，答完自动切下一条，条上提示「还有 N 条待回答」。
- 提交后进入「已选择 X」态并**保留 2 秒**（`ASK_USER_BANNER_LINGER_MS`，从提交成功起算，避免 tool call 立刻变 success 导致提前卸载）；linger 期间若出现**提交时不在队列里**的新 pending，立即替换确认态。
- 交互与卡片一致：单选点击即提交；多选保留确认按钮；「其他」输入保留。IM 不受影响（走 `im_ask_user` 推送）。
- 旧逻辑已剔除：`isInteractiveToolCall` 拆为 `isPendingApprovalToolCall` + `isPendingAskUserToolCall`；`agentTraceNeedsCollapsedSurface` 只按 `status === 'running'`；折叠帧只保留 approval 卡片；`SubAgentFrameHost` 的 stub 降级判据改为 `approvalBlocksStub`。`ToolCallRow` 仍在工具行自身渲染处显示 `ask_user` 卡片（展开帧中与顶部条并存，属既有渲染路径）。

## `ask_user` 工具行

- 工具行标题只显示 **图标 +「询问用户」**（不加 `displaySummary` / 问题摘要）。
- **问题**写在围栏 **header**（表格 `th` 同款：`--hover` 底、底部分隔线），选项在 header 下方（`--card`，与表格 `td` 一致）。
- 问题与选项包在 **`.fence-block`** 里：外框与表格相同（`rounded-lg border-border`），不要做成白底扁平列表。
- 卡片与工具行同一左缘（不要 `ml-4`）。问题、选项勾选、「其他」与表头一样走 **`px-3` 左内边距**，选项不要 `-mx-1` 把左缘拉偏。
- 宿主固定提供「其他」自由输入（对齐 Hermes）；不必要求模型在 `options` 里加 Other。
- 选项文案（含 description）在卡片宽度内**自动换行**，不要用 `truncate` 单行截断。
- 「其他」输入框设 `max-w-[14rem]`，不要 `flex-1` 拉满整行。
- 选中态只用主题语义色：`foreground` / `background` / `border` / `hover` / `muted`（随浅色/深色翻转）。
  勾选填充为 `bg-foreground/55 text-background`，不要写死灰阶、紫色或未定义的 `muted-foreground`。
- **选完之后的工具间隙**：`ask_user` 不可并进工具组，完成后会落在列表末尾。须在其后保留 live「思考中.」槽（主会话 `collapsedToolListItems` 尾部空组；子 Agent 统计行第二行，**展开过程时也要留**思考间隙，不要只在收缩态显示）。用户不应在选完选项后长时间既无思考提示、也无下一工具。
- **进行中的子 Agent / 后台宿主**：父列表里若有 in-flight 的 `run_subagent`（或 live 后台宿主），`parentThinkingSuppressedByHost` 为真时**不要**再挂父级「思考中」（也不要追加尾部空组）——`SubAgentFrame` 内已有 live / 思考面，外层再叠一行会出现在宿主下方、与当前子任务工具行并列的重复「思考中」。
- **子 Agent**：询问卡片改由对话区顶部固定条（`AskUserBanner`）承载，帧内不再为露出 `ask_user` 保留过程/宿主行。帧里若仍渲染到 `ask_user` 工具行，卡片照常可答（既有渲染路径）；scoped 行和 session 上的同一次调用要合并，留下能画出选项的那一份（scoped 空参数不能挡住 session）。
- **参数**：schema 里 `options` 已是 `type: array`；界面和执行端都兼容模型把数组二次字符串化的写法（解析 JSON 数组；容忍尾部多余 `]`）。工具文档明确要求传原生数组，不要传字符串。

## 上下文压缩进行中标记

- 事件：`context_compression_started` → 会话 run state 的 `contextCompressing`。
- 展示：插在摘要切分点（`insertBeforeMessageId` 之前），工具行样式（`ContextCompressingMarker`），含旋转「压缩中」。切点在过程行上时，收缩态把标记留在该问最终回复之前（问题 → 压缩中 → 回复），不随「工作」藏掉。切点不在当前分页窗口时，插在本窗口第一问的回复之前，不要落到正在跑的回合末尾。
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
