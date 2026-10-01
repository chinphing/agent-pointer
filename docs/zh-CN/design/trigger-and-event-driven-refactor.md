# Trigger & Event-Driven Refactor 设计

将 pointer 从「直接函数调用」改造为「可调用 / 事件触发」的统一调度机制。所有触发源汇总到 `RunDispatcher::dispatch`，复用既有的 `run_chat` 核心，并新增队列、幂等、事件总线、生命周期钩子、HTTP Runs API、Webhook、Cron 调度器。

## 目标

1. **单一入口**：IPC / HTTP / Webhook / Cron / IM / 内部事件都构造 `TriggerRequest` 走 `dispatch`。
2. **可观测**：`runs` 表持久化每次 run 的状态迁移；`AgentEventBus` 广播结构化、按 run 排序的事件。
3. **可扩展**：`HookRegistry` 在生命周期点支持 rewrite / reject / observation。
4. **有序与限流**：lane（会话）串行 + 全局并发上限，避免并发回合损坏历史。
5. **幂等**：`idempotency_key` 命中已有 run 时返回 `Reused`，不重复执行。
6. **跨端兼容**：dispatcher 内嵌于既有 Tauri / server 进程，桌面与 web 两端共用同一核心。

## 分阶段交付

| 阶段 | 内容 | 状态 |
|------|------|------|
| 1 | dispatcher / trigger / queue + agent_events + `runs` 表（schema v7） | 完成 |
| 2 | HookRegistry + 6 生命周期钩子接线 + 内置 `LifecycleLogHook` | 完成 |
| 3 | 迁移 Tauri `send_chat` / server `POST /api/chat` 到 dispatcher；`pre/post_tool_call` 发射点暂缓 | 完成（IM 与 tool 钩子暂缓，见下） |
| 4 | HTTP Runs API（`POST /api/runs` + SSE + cancel）+ 通用 Webhook（Bearer） | 完成 |
| 5 | scheduler 模块 + `cron_jobs` 表（schema v8）+ ticker，桌面默认关 / web 默认开 | 完成 |
| 6 | 内部触发迁移 + 文档 | 部分完成（见下） |

## 关键设计

### RunDispatcher
持有 `Arc<AppState>`、`RunQueue`、`AgentEventBus`、`HookRegistry`、`CancellationToken` 表。`dispatch` 编排：钩子 → 幂等 → 解析 id → 入库 → 排队 → spawn runner。`run_runner` 获取许可后调 `run_chat`，用 `stream_event_to_agent_event` 把 `StreamEvent` 桥接到 `AgentEvent`，终态写库 + 触发终态钩子。

### TriggerRequest
统一请求结构，字段对齐 `SendChatPayload` + 触发元数据 + `DeliverTarget`。`messages` 沿用 `run_chat` 契约（调用方提供完整回合历史）。

### RunQueue
lane 默认 = `conversation_id`，保证同一会话回合严格串行；全局 `Semaphore`（默认 4）限并发。`LaneGuard` RAII 释放资源。

### AgentEventBus
`tokio::sync::broadcast`，按 `run_id` 分配 `seq`。与老的 `stream_broadcast`（UI 全局通道）并存：`run_chat` 仍发 `stream_broadcast`，dispatcher 额外桥接到 `AgentEventBus` 供新订阅者。

### 幂等
`runs` 表 `idempotency_key` 唯一索引。命中且非终态时返回 `Reused`。Cron 用 `cron:{id}:{scheduled_ms}` 作幂等键，避免同槽位重复触发。

## 范围与取舍（重要）

以下项经评估后**暂缓**，原因记录于此以便后续决策：

1. **IM 入站不走 dispatcher**：`pointer-channels` 的 IM dispatch 直接消费 `StreamEvent` 流来拼装回复，是已成熟的事件驱动路径。改走 dispatcher 需把回复收集重写为消费 `AgentEvent`，收益低、回归风险高。
2. **内部后台任务（curator / memory review）未迁移到 `dispatch_internal`**：这两者是定制 LLM 调用，不走 `run_chat`，与 dispatcher 的「会话回合」契约不匹配。强行迁移需把它们重构成会话回合形态或扩展 dispatcher 支持非 `run_chat` runner，属高风险大改。`dispatch_internal` 已就绪，供未来「会话回合型」内部触发使用。
3. **`pre/post_tool_call` 发射点未接入 `agent_tool_pass`**：目前无消费者，接入需把 `HookRegistry` + dispatcher `run_id` 线程到工具执行上下文，触碰 `run_chat` 签名，风险高。待有真实消费者再做。

这三项均不阻塞「可调用 / 事件触发」主目标——IPC、HTTP Runs API、Webhook、Cron 已全部走 dispatcher。

## 跨端 / 跨入口兼容

- **桌面（Tauri）**：`send_chat` 走 dispatcher；Cron 默认开（`POINTER_SCHEDULER_ENABLED=0` 可关）。
- **Web（server）**：`POST /api/chat` 走 dispatcher；`POST /api/runs` + Webhook + Cron 默认开。
- 两端共用 `pointer-core` 的 dispatcher / queue / hooks / runs 表，行为一致。
