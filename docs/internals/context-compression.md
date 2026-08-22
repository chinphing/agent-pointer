# 上下文压缩可靠性

本文说明主会话与子 Agent 的上下文压缩边界、摘要输入预算、
摘要验收和持久化重载约定。

## 触发与保留边界

- **发送路径不主动同步压缩**，也不 await 后台预压缩。
- 工具轮次耗尽时可强制尝试压缩（同步）。
- **子 Agent** 与主会话共用 80% / 硬预算 / in-run 门闩，作用在隔离的 `local_history`；
  软阈值后台预压（独立队列），硬预算同步；不占用 lead 预压队列。
- **保留区按消息条数**（约工作集的 20%，至少 3 条、且至少留 1 条可压）。
  上一轮即使极长，只要落在条数尾部之外就可以被摘要。
  最新一条真实用户消息始终保留原文。
- **路径选择（消息条数）**：从最新一条真实用户消息到列表末尾占工作集 **≥ 70%** 时走 **run 内压缩**；否则走原来的前缀摘要。
  Run 内只 soft-exclude 该用户消息之后、条数尾之前的过程。
  切点对齐 assistant 与其后 `role: tool` 整组（含切点落在该组**第一条** tool 行：
  必须把 assistant 拉进保留区，避免摘要后出现孤儿 tool）。
  摘要插在尾部之前。
  摘要模型看到「尾部以外的全文」作参考，但只叙述被摘掉的那一段（Progress / State / Next）。
- 自动重试提示和仅表示继续执行的短消息不占用用户轮次保留名额。
- **不截断**保留区内的工具输出或 assistant 正文。缩小上下文只靠把前缀或当前轮中段换成摘要。

## 用户可调参数

**设置 → 智能体 → 上下文自动压缩** 里只保留：

- 开关 `contextCompressionEnabled`
- **上下文预算** `contextBudgetTokens`（硬阈值；软预压缩约 80%；**何时压**仍看 token。原文尾部按条数约 20%，超限约 12%）
  设置页按 **KB** 编辑，默认 **256KB**（落盘 `256 × 1024`）。
  读到旧默认 `100000` / `120000`（token 时代）时按 256KB 处理。

`contextKeepRecentUserTurns` 仍写入用户配置和压缩事件，但**不再**作为切分地板。
`contextSummaryMaxTokens` 不参与对话压缩（摘要长度按前缀动态计算），后台 review 仍可能用到。

## 同轮超限恢复

Provider 在本轮工具循环中返回上下文过长时：

1. 按更紧的条数尾部（约 12%）强制摘要压缩（忽略可压占比）。
2. **同一轮 LLM 循环内重试**（最多 2 次），不要求用户重发，也不因已经流式过而放弃。
3. 主会话入口仍保留「尚未产生 assistant 输出则整轮重试」作为兜底。
4. 无法缩小则明确报错，避免空转。
  若当前轮已占工作集条数 ≥ 70%，overflow 也会走 run 内压缩（更紧的 12% 条数尾）。
  仅当 run 内窗口为空且前缀切分点为 0 时，不能靠截断工具行腾空间。

## 异步预压缩（软阈值 + 闲时落盘）

目标：接近硬预算时后台先压，**发送路径和下一次 LLM 调用都不等待**。

- 软阈值：`gate > context_budget_tokens × 0.80`。
  `gate` 优先用上一轮 `usage.prompt_tokens`；没有 usage 才本地估算。
  前缀路径另要求可压占比 ≥ 0.30；当前轮已占 **≥ 70% 条数** 时改为 run 内压缩，不要求该占比。
- 触发点：
  - **主会话**每一轮 LLM **之前**（同一用户回合的工具循环中间也可以）；
  - **子 Agent** 每一轮 LLM **之前**（隔离 `local_history`，同一套 80% / 硬预算与 in-run 判定）；
  - 一轮 `run_chat` 成功结束之后（只压 **lead** 工作集）。
- 主会话摘要可在后台完成：若该会话仍有活跃回合，结果 **入队**；
  下一轮 LLM 开始前 `try_apply_pending_compression_live` 接到当前工作集
  （切分点之后新追加的消息不判过期；run 内切分点是条数尾起点）。
  **接入 splice 后本轮不再用上一轮 `usage.prompt_tokens`**，门闩按压完后的
  history 本地估算；真的还超才同步再压。软预压 spawn 也用失效后的 token。
  丢弃窗口里没有可压消息（计入上下文且非合成用户行）时不发「压缩中」、
  不落第二条摘要。
- 仅当已经超过 **硬预算** 时主会话才同步等待（先等正在跑的后台任务，不够再当场压缩）。
- **子 Agent 预压队列与 lead 隔离**（`sub:{conversationId}:{agentInstanceId}`）。
  过 80% 时后台摘要、下一轮 LLM 前接入；硬预算则等待 inflight 再同步。
  子循环结束丢弃未接入的 pending，避免误压父聊天。
- 预压缩仍可广播 UI 事件。

## 上下文超限（阻塞路径）

Provider 返回上下文/prompt 过长类错误时：

1. 丢弃同会话 pending splice；
2. 同步 `recover_history_after_overflow`（忽略可压占比 + 更紧条数尾部，摘要替换前缀）；
3. **优先在当前工具循环内重试 LLM**（主会话与子 Agent 均如此）；
4. 若错误冒泡到 `run_chat` 且本回合尚未产生 assistant 输出 → 再整轮重试一次；
5. 若无法缩小 → 明确错误提示，不空转。

## 摘要输入预算

压缩只读取 `context_state.included=true` 的旧前缀消息。
已被历史压缩或任务板 trim 排除的数据库行不会再次进入摘要。

当格式化后的前缀超过输入上限时：

1. 保留最新一条旧摘要；没有旧摘要时保留最初的真实用户目标。
2. 从压缩 split 位点向前倒序使用剩余预算。
3. 最终按原时间顺序发送所选消息块。
4. 在跳过区域写明省略的消息块数量。

该策略只影响**送给摘要模型的格式化输入**，不改写会话里保存的原文。
紧邻压缩位点的任务进展、错误和未完成事项优先于中间的大段工具输出，
不允许简单保留最早字符并丢弃前缀尾部。

摘要请求将历史放在明确的 source conversation 边界内。
边界前后都要求模型只生成摘要、不得回答或继续历史中的请求，
完整章节模板位于历史之后，避免长输入的近因内容覆盖摘要任务。

## 摘要输出预算

摘要默认 **关闭 thinking**。第一次请求的 API `max_tokens` 是预算的 **1.5 倍**
（预算仍写进 prompt 当目标长度）。若 `finish_reason=length`，再按预算
**×3** 重试一次；仍失败则走下方失败兜底。

```
summary_max_tokens = clamp(
  content_tokens × 0.20,
  floor = 1_500,
  ceiling = 12_000
)
requested_max_tokens = summary_max_tokens × 1.5
retry_max_tokens     = summary_max_tokens × 3
```

关思考走与主对话相同的协议翻译：千问 `enable_thinking=false`，
DeepSeek 去掉 `reasoning_effort`（不写 `thinking.type`）。

## 摘要用哪个模型

摘要走 **`chat_once_without_thinking`**，模型与 **当前这场对话 lead 已解析的 provider/model** 相同（含请求上的 lead / 性能档），不另选压缩模型。

- 工具循环内压缩、超限恢复：直接用该轮 `OpenAIProvider`。
- 后台预压缩：使用 `run_chat` 开始时记下的会话 LLM（`remember_session_llm`），**不再** `effective_settings()` + 空 override 重解析（否则可能打到另一家网关）。
- 没有会话 LLM 记录时跳过预压缩并 warn，避免静默换模型。

## 摘要验收与失败语义

摘要必须满足：

- `finish_reason` 为空或 `stop`；
- 输出正文非空。

约定章节用于引导组织，不作为逐字匹配的验收条件。

- 前缀：Goal / Progress / State / Open
- Run 内：Progress / State / Next（用户原话仍在上下文，不写 Goal）

Progress 是时间线（已做、阻塞、按议题决定）。
State 是快照（目录/文件、环境变量、约定文案、接口与命令名等字面值），
不重复写决定。
Open / Next 是还没做完的：跨轮待问用 Open，本轮未完成用 Next。

摘要要带走仍有效的约束和按议题保留的最新决定
（不是整段对话只留最后一条）；后续轮次没再提起不等于可以丢。
只有同一议题被明确替代才替换。
超预算时，决定与 State 字面值的优先级不低于 blockers。

写回上下文的摘要保留稳定识别前缀，并追加 `REFERENCE ONLY` 说明。

摘要尝试失败时：

- **丢弃**窗口里除兜底保留之外的消息（soft-exclude + drain）；
- **保留最近 3 条真实用户消息**，以及每条用户消息对应的
  **收尾助手消息**（该回合最后一条无后续 `role: tool`、且 tool_calls 已结束的 assistant）；
- 写入确定性 handoff 摘要（说明丢弃了什么、哪些回合仍保留原文）；
- warning toast；
- `reason` 记为 `budget_drop` / `tool_limit_drop` / `overflow_drop` / `in_run_drop`。

收尾助手若仍带着未完成的 tool_calls 或后面还有 tool 行，则只保留用户原文，不保留该助手行，避免孤儿 tool。
工具过程与更早的用户回合仍丢弃。成功压缩路径不变，仍按条数尾部切分。

## 可压占比（仅本地兜底）

有上一轮 `usage.prompt_tokens` 时：**只比较该值与软/硬阈值**，不再扫一遍消息估 token。
此时只要存在可摘要前缀（最新真实用户消息不在第 0 条）即可触发。
本轮已经接入 pending splice 后不再沿用该 usage，改为本地估算。

没有 usage 时才走本地启发式，并要求
`prefix_payload / payload_est ≥ 0.30`。

## 持久化与跨入口一致性

SQLite 是已有消息顺序和 `context_state` 的权威来源。
每次 `run_chat` 开始时：

1. 调用方历史仅用于追加数据库中不存在的新消息 ID；
2. 经 `conversation_session::prepare_lead_history` 取得 **lead LLM 工作集**
   （优先进程内 cache；miss 时按 `context_included = 1` 从库加载）；
3. soft-excluded 行留在 SQLite，供 UI 分页 hydrate；会话
   `message_count` 仍为全库行数；
4. APP、WEB、IM 的 `run_chat` 内存历史均使用该工作集；落盘 sync
   在存在 DB orphan 时走保位路径，不会因短列表删掉 excluded 行。
   压缩 drain 后需 `publish_working_set` 刷新 cache。

详见 [`conversation-session.md`](conversation-session.md)。

压缩成功后必须清除 `last_lead_prompt_tokens`。

## UI 进度标记与摘要位置

压缩开始摘要 LLM 时发送 **`context_compression_started`**（不落库），
带切分点 `insertBeforeMessageId`。进行中灰字插在该消息之前。

摘要行落在切分点（保留区第一条之前）。前端按记录的 `insertBeforeMessageId` 插入，
不把切点从 tool/glue 扫到下一条可见回复。

- **前缀压缩**：摘要仍是 `user` 行，但不是用户任务回合。
  切点是真实用户消息时：芯片挂在该问题回合头，收缩后仍和问题一起可见。
  切点落在 tool / assistant 时：展示与 run 内相同（过程中间的「自动压缩摘要」，
  收缩时随「工作」隐藏）。摘要正文与丢弃区间仍按前缀路径，不改成 in-run。
- **Run 内压缩**：摘要是 `assistant` 行，插在当前用户回合的过程中间，不新开一轮、不拆耗时条。
  芯片样式与前缀相同（`自动压缩摘要`），不当作模型正文回复。
  回合收缩时随工具过程一起藏进「工作」，不单独留在折叠条下面。
  思考模式线路仍发 `role: assistant`，并带合成 `reasoning_content`（`[context compression]`）；
  已落盘、reasoning 为空的摘要也在组请求时补上，避免 DeepSeek 400。

找不到切点 id 时：摘要插在第一条未被排除的消息之前；
进行中标记插在该回合最后一条可见回复之前；落库插在最后一条 excluded 行之后。

完成后仍用 **`UiToast`** 提示结果。

## 代码布局

`crates/pointer-core/src/context_compression/`（对外路径仍是 `crate::context_compression`）：

| 文件 | 职责 |
|------|------|
| `budget.rs` | token 估算、80%/硬门闩、前缀 vs in-run 切分 |
| `summary.rs` | 摘要输入格式化、prompt、验收与落盘正文 |
| `run.rs` | 同步压缩（预算/工具轮耗尽/超限恢复） |
| `precompress.rs` | 后台预压、pending splice、轮间 prepare |
| `types.rs` | scope / UI 上下文 / 子实例队列键 |

## 可观测性

关键日志字段：`total` / `prefix` / `ratio` / `threshold` / `soft`，
以及 pending enqueue / apply / stale discard、overflow retry。
