# 上下文压缩可靠性

本文说明主会话与子 Agent 的上下文压缩边界、摘要输入预算、
摘要验收和持久化重载约定。

## 触发与保留边界

- **发送路径不主动同步压缩**，也不 await 后台预压缩。
- 工具轮次耗尽时可强制尝试压缩（同步）。
- 最近若干个有独立语义的用户轮次及其后续消息保持原文。
- 自动重试提示和仅表示继续执行的短消息不占用用户轮次保留名额。

## 异步预压缩（软阈值 + 闲时落盘）

目标：接近硬预算时后台先压，发送路径零等待。

- 软阈值：`total_gate > context_budget_tokens × 0.80`。
- **可压占比**：`prefix_payload / payload_est ≥ 0.30`（payload 同口径；
  `prefix` = 排除 `contextKeepRecentUserTurns` 保留区后的可压窗口），否则不触发，
  避免「体积在保留区 → 压完几乎不掉量 → 马上再压」。超预算判定仍用
  `gate = max(api_prompt, payload_est)`。
- 一轮 `run_chat` 成功结束后，若满足上述条件且无同会话 inflight，则后台异步压缩。
- 若摘要完成时该会话仍有活跃回合：结果 **入队**，不写 SQLite；回合结束后
  `try_apply_pending_compression` 在指纹仍匹配时落盘，过期则丢弃并可能再调度预压。
- 预压缩仍可广播 UI 事件。

## 上下文超限（唯一阻塞压缩）

仅当 provider 返回上下文/prompt 过长类错误时：

1. 丢弃同会话 pending splice；
2. 同步 `maybe_compress_history`（同样受可压占比约束）；
3. 若本回合尚未产生 assistant 流式输出且压缩成功 → **同一次 `run_chat` 内自动重试**；
4. 若已流式 → 只压缩落盘，提示用户重发；
5. 若无法压缩（无可压前缀 / 占比不足）→ 明确错误提示，不空转。

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
- 验收失败则直接走 drop handoff，不再放大预算重试。

## 摘要验收与失败语义

摘要必须满足：

- `finish_reason` 为空或 `stop`；
- 输出正文非空。

约定章节用于引导模型组织内容，不作为逐字匹配的验收条件。

写回上下文的摘要保留稳定识别前缀，并追加 `REFERENCE ONLY` 说明。

摘要尝试失败时：

- **丢弃**待压缩前缀（soft-exclude + drain）；
- 写入确定性 handoff 摘要；
- warning toast；
- `reason` 记为 `budget_drop` / `tool_limit_drop`。

## 可压占比（payload 同口径）

触发条件：`gate_tokens > soft/hard 阈值` **且**
`prefix_payload / payload_est ≥ 0.30`。

- `gate_tokens` 仍取 `max(api_prompt, payload_est)`（是否超预算）。
- **ratio 只除 payload**：避免系统提示/工具 schema 把 API prompt 抬高后误判「可压占比不足」而漏触发。

## 持久化与跨入口一致性

SQLite 是已有消息顺序和 `context_state` 的权威来源。
每次 `run_chat` 开始时：

1. 调用方历史仅用于追加数据库中不存在的新消息 ID；
2. 后端重新加载数据库顺序；
3. APP、WEB、IM 后续均使用该规范化历史。

压缩成功后必须清除 `last_lead_prompt_tokens`。

## UI 进度标记

压缩真正开始摘要 LLM 时发送 **`context_compression_started`**（临时事件，不落库）。

完成后仍用 **`UiToast`** 提示压缩结果（成功或摘要失败后的 drop）。

## 可观测性

关键日志字段：`total` / `prefix` / `ratio` / `threshold` / `soft`，
以及 pending enqueue / apply / stale discard、overflow retry。
