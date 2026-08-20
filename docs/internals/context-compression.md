# 上下文压缩可靠性

本文说明主会话与子 Agent 的上下文压缩边界、摘要输入预算、
摘要验收和持久化重载约定。

## 触发与保留边界

- **发送路径不主动同步压缩**，也不 await 后台预压缩。
- 工具轮次耗尽时可强制尝试压缩（同步）。
- **保留区按 token 预算**（约上下文窗口的 20%），而不是固定「最近 N 轮用户」。
  上一轮即使极长，只要落在 token 尾部之外就可以被摘要。
  最新一条真实用户消息始终保留原文。
- 自动重试提示和仅表示继续执行的短消息不占用用户轮次保留名额。
- **不截断**保留区内的工具输出或 assistant 正文。缩小上下文只靠把前缀换成摘要。
  当前用户消息之后、仍落在保留区内的工具行保持原文。

## 用户可调参数

系统设置里只保留：

- 开关 `contextCompressionEnabled`
- **上下文预算** `contextBudgetTokens`（硬阈值；软预压缩约 80%；原文尾部约 20%，超限约 12%）

`contextKeepRecentUserTurns` 仍写入用户配置和压缩事件，但**不再**作为切分地板。
`contextSummaryMaxTokens` 不参与对话压缩（摘要长度按前缀动态计算），后台 review 仍可能用到。

## 同轮超限恢复

Provider 在本轮工具循环中返回上下文过长时：

1. 按更紧的 token 尾部（约 12%）强制摘要压缩（忽略可压占比）。
2. **同一轮 LLM 循环内重试**（最多 2 次），不要求用户重发，也不因已经流式过而放弃。
3. 主会话入口仍保留「尚未产生 assistant 输出则整轮重试」作为兜底。
4. 无法缩小则明确报错，避免空转。
  若膨胀几乎全在当前用户消息之后，摘要切分点可能为 0，此时不能靠截断工具行来腾空间。

## 异步预压缩（软阈值 + 闲时落盘）

目标：接近硬预算时后台先压，**发送路径和下一次 LLM 调用都不等待**。

- 软阈值：`gate > context_budget_tokens × 0.80`。
  `gate` 优先用上一轮 `usage.prompt_tokens`；没有 usage 才本地估算，
  并要求可压占比 ≥ 0.30。
- 触发点：
  - 每一轮 LLM **之前**（同一用户回合的工具循环中间也可以）；
  - 一轮 `run_chat` 成功结束之后。
- 摘要在后台完成：若该会话仍有活跃回合，结果 **入队**；
  下一轮 LLM 开始前 `try_apply_pending_compression_live` 在前缀指纹仍匹配时
  接到当前工作集（新工具消息只追加在切分点之后，不会判过期）。
- 仅当已经超过 **硬预算** 时才同步等待（先等正在跑的后台任务，不够再当场压缩）。
- 预压缩仍可广播 UI 事件。

## 上下文超限（阻塞路径）

Provider 返回上下文/prompt 过长类错误时：

1. 丢弃同会话 pending splice；
2. 同步 `recover_history_after_overflow`（忽略可压占比 + 更紧 token 尾部，摘要替换前缀）；
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

摘要默认 **关闭 thinking**，**单次**调用（不重试）：

```
summary_max_tokens = clamp(
  content_tokens × 0.20,
  floor = 1_500,
  ceiling = 12_000
)
```

- 使用 `chat_once_without_thinking`。
- 关思考走与主对话相同的协议翻译：千问 `enable_thinking=false`，
  DeepSeek 去掉 `reasoning_effort`（不写 `thinking.type`）。
- 验收失败则直接走 drop handoff，不再放大预算重试。

## 摘要验收与失败语义

摘要必须满足：

- `finish_reason` 为空或 `stop`；
- 输出正文非空。

约定四个章节（Goal / Progress / State / Open）用于引导组织，
不作为逐字匹配的验收条件。

写回上下文的摘要保留稳定识别前缀，并追加 `REFERENCE ONLY` 说明。

摘要尝试失败时：

- **丢弃**待压缩前缀（soft-exclude + drain）；
- 写入确定性 handoff 摘要；
- warning toast；
- `reason` 记为 `budget_drop` / `tool_limit_drop` / `overflow_drop`。

## 可压占比（仅本地兜底）

有上一轮 `usage.prompt_tokens` 时：**只比较该值与软/硬阈值**，不再扫一遍消息估 token。
此时只要存在可摘要前缀（最新真实用户消息不在第 0 条）即可触发。

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

## UI 进度标记

压缩真正开始摘要 LLM 时发送 **`context_compression_started`**（临时事件，不落库）。

完成后仍用 **`UiToast`** 提示压缩结果（成功或摘要失败后的 drop）。

## 可观测性

关键日志字段：`total` / `prefix` / `ratio` / `threshold` / `soft`，
以及 pending enqueue / apply / stale discard、overflow retry。
