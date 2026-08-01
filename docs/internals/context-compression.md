# 上下文压缩可靠性

本文说明主会话与子 Agent 的上下文压缩边界、摘要输入预算、
摘要验收和持久化重载约定。

## 触发与保留边界

- 主会话在每个新用户轮次开始时按 token 预算判断是否压缩。
- 工具轮次耗尽时可强制尝试压缩。
- 最近若干个有独立语义的用户轮次及其后续消息保持原文。
- 自动重试提示和仅表示继续执行的短消息不占用用户轮次保留名额。

## 摘要输入预算

压缩只读取 `context_state.included=true` 的旧前缀消息。
已被历史压缩或任务板 trim 排除的数据库行不会再次进入摘要。

当格式化后的前缀超过输入上限时：

1. 保留最新一条旧摘要；没有旧摘要时保留最初的真实用户目标。
2. 从压缩 split 位点向前倒序使用剩余预算。
3. 最终按原时间顺序发送所选消息块。
4. 在跳过区域写明省略的消息块数量。

该策略保证紧邻压缩位点的任务进展、错误和未完成事项优先于中间的
大段工具输出，不允许简单保留最早字符并丢弃前缀尾部。

摘要请求将历史放在明确的 source conversation 边界内。
边界前后都要求模型只生成摘要、不得回答或继续历史中的请求，
完整章节模板位于历史之后，避免长输入的近因内容覆盖摘要任务。

## 摘要输出预算（对齐 Hermes）

摘要 `max_tokens` 按被压缩内容动态估算（设置页不再暴露摘要 tokens）：

```
summary_max_tokens = clamp(
  content_tokens × 0.10,
  floor = 2000,
  ceiling = 16_000
)
```

- `content_tokens`：格式化后待摘要前缀的启发式 token 估算。
- 第一次失败后重试一次（关闭 thinking），`max_tokens × 1.5`，重试上限 **24_000**。

## 摘要验收与失败语义

摘要必须满足：

- `finish_reason` 为空或 `stop`；
- 输出正文非空。

约定章节用于引导模型组织内容，不作为逐字匹配的验收条件。
首次输出被截断或为空时，使用禁用 thinking 的辅助请求重试一次（并放大输出预算），
避免隐藏推理占用摘要输出 token。

写回上下文的摘要保留稳定识别前缀，并追加 `REFERENCE ONLY` 说明。
后续模型必须将摘要视为背景资料，只处理摘要之后的新消息。

两次尝试均失败时（对齐 Hermes 默认行为）：

- **丢弃**待压缩前缀（soft-exclude + drain）；
- 写入一条确定性 handoff 摘要（标明 summary unavailable / dropped N）；
- 发出 warning toast（「摘要生成失败，已丢弃较早…」）；
- `reason` 记为 `budget_drop` / `tool_limit_drop`。

不再在摘要失败时原样保留超预算历史（那会导致下一轮继续撞墙）。

## 持久化与跨入口一致性

SQLite 是已有消息顺序和 `context_state` 的权威来源。
每次 `run_chat` 开始时：

1. 调用方历史仅用于追加数据库中不存在的新消息 ID；
2. 后端重新加载数据库顺序；
3. APP、WEB、IM 后续均使用该规范化历史。

APP 和 WEB 在项目切换、空 shell、消息淘汰或 IM fork 后发送前，
必须等待 hydration 完成。加载失败时阻止发送并保留待发送消息。

会话已 hydration 后，前端 `sendChat` 与 `persistAppend`（回合结束 / trim /
发送失败兜底）都只序列化尚未写入 SQLite 的新消息，不再深拷贝整段历史。
水位线随 hydration、`sendChat` 成功与 `persistAppend` 更新；`sendChat` 若增量
结果为空则回退全量，`persistAppend` 增量为空则跳过 append（仍刷新水位线）。

压缩成功后，无论由预算还是工具轮次触发，都必须清除
`last_lead_prompt_tokens`，防止下一轮使用压缩前的陈旧 token 数。

## UI 进度标记

压缩真正开始摘要 LLM 时发送 **`context_compression_started`**（临时事件，不落库）。

前端在消息列表工具行区域显示「正在压缩较早记录 / 压缩中」动态标记；
在 `context_compression_applied`、`context_compressed`、停止或 `done`/`error` 清 run state 时自动隐藏。

完成后仍用 **`UiToast`** 提示压缩结果（成功或摘要失败后的 drop）。

## 可观测性

- 成功日志包含 split、输入估算、API prompt tokens、格式化耗时和摘要耗时。
- 每次压缩记录 `summary_budget` / `summary_retry_budget`（content_tokens、max_tokens）。
- 每次被拒绝的摘要记录 attempt、拒绝原因、finish reason、max_tokens 和 token 用量。
- 两次失败后记录 `drop_without_summary`，并仍走 splice 落库 / UI 事件。
- 运行开始时记录数据库规范化后的消息数。
