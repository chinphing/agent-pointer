# Trigger Dispatcher 与事件驱动运行时

`pointer-core` 的统一可调用 / 事件触发入口。所有触发源（IPC、HTTP Runs API、Webhook、Cron、IM、内部事件）都构造一个 `TriggerRequest` 并交给 `RunDispatcher::dispatch`，由它排队、幂等校验、持久化、启动 `run_chat`、桥接事件并触发生命周期钩子。

> 设计参考：hermes（单核心 + gateway 适配 + 插件钩子）、openclaw（异步 ack / stream / final RPC + lane 队列 + 两层钩子 + cron）。

## 模块位置

| 模块 | 职责 |
|------|------|
| `crates/pointer-core/src/dispatcher/mod.rs` | `RunDispatcher`：dispatch / run_runner / cancel / wait / subscribe_events / run_status / dispatch_internal |
| `crates/pointer-core/src/dispatcher/trigger.rs` | `TriggerRequest`、`TriggerSource`、`TriggerMeta`、`DeliverTarget`、`RunHandle`、`RunAcceptStatus`、`RunOutcome` |
| `crates/pointer-core/src/dispatcher/queue.rs` | `RunQueue`：lane 串行 + 全局并发上限（`Semaphore`） |
| `crates/pointer-core/src/dispatcher/hooks.rs` | `HookRegistry` + 8 个生命周期钩子 trait + 内置 `LifecycleLogHook` |
| `crates/pointer-core/src/agent_events.rs` | `AgentEvent` + `AgentEventBus`（`broadcast`）+ `StreamEvent`→`AgentEvent` 桥接 |
| `crates/pointer-core/src/scheduler.rs` | Cron 调度器：ticker + `cron_jobs` 表轮询 + `dispatch` |
| `crates/pointer-core/src/conversation_store/runs.rs` | `runs` 表（schema v7）：run 状态 + 幂等键 |
| `crates/pointer-core/src/conversation_store/cron_jobs.rs` | `cron_jobs` 表（schema v8）：定时任务 |

## 触发源（`TriggerSource`）

| 变体 | 来源 | 接入点 |
|------|------|--------|
| `Ipc` | Tauri `send_chat` | `src-tauri/src/commands.rs` → `dispatcher.dispatch` |
| `HttpRuns` | `POST /api/runs`（web / 外部 REST） | `server/src/main.rs::create_run` |
| `Webhook` | `POST /api/webhooks/:src`（Bearer） | `server/src/main.rs::webhook_ingress` — 见 [`../developer/webhook-api.md`](../developer/webhook-api.md) |
| `Cron` | scheduler ticker | `scheduler.rs::dispatch_job` |
| `Im` | IM 入站消息（飞书 / 企微 / 钉钉 / 微信） | `pointer-channels`（仍直连 `run_chat`，见下） |
| `Internal` | 内部后台任务 | `dispatch_internal`（预留；见下） |

## 一次 dispatch 的生命周期

1. `on_trigger_received` 钩子（可 rewrite / reject）
2. 解析 `conversation_id`（`None` 时由触发源派生）
3. 幂等校验：`runs` 表按 `idempotency_key` 查重，命中则返回 `Reused`
4. 解析 `run_id`（`None` 时生成 uuid v4）
5. `pre_dispatch` 钩子（可 reject）
6. `runs` 表插入 `queued`，emit `AgentEvent::RunQueued`
7. 注册 `CancellationToken`，spawn `run_runner`
8. `run_runner`：acquire lane/全局许可 → `running` + emit `RunStarted` + `on_run_started` 钩子 → 桥接 `StreamEvent`→`AgentEvent` → 调 `run_chat` → 终态 `finalize_terminal`（`on_run_finished` / `on_run_failed` / `on_run_cancelled`）

## 事件总线

`AgentEventBus` 是 `tokio::sync::broadcast`，按 `run_id` 分配单调 `seq`。订阅者只能收到订阅之后的事件；历史状态查 `runs` 表。`run_chat` 仍走老的 `stream_broadcast`（UI 全局通道），dispatcher 额外把 `StreamEvent` 桥接成 `AgentEvent` 发到总线，供新订阅者（HTTP SSE / SDK）消费。

## HTTP Runs API（server）

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/runs` | body = `TriggerRequest`（`trigger_source` 强制为 `HttpRuns`）→ 202 + `RunHandle` |
| `GET` | `/api/runs/:id` | run 状态快照（`RunView`），404 if 未知 |
| `GET` | `/api/runs/:id/events` | SSE，按 `run_id` 过滤的 `AgentEvent` 流；终态前已结束则用 `runs` 表合成终态帧后关闭 |
| `POST` | `/api/runs/:id/cancel` | 取消排队 / 运行中的 run，204 |

SSE 终态处理是无竞态的：先订阅总线，再读 `runs` 表；已终态则合成帧关闭，否则流到终态帧关闭。

## 通用 Webhook（server）

Webhook 对外集成说明见 **[`../developer/webhook-api.md`](../developer/webhook-api.md)**。

## Cron 调度器

- `cron_jobs` 表（schema v8）：`id / label / cron_expr / conversation_id / prompt_text / agent_mode / lead_agent_id / enabled / last_run_at_ms / next_run_at_ms / created_at_ms`。
- ticker 每 60s 轮询 `cron_jobs_list_due`，对每个到期任务 `dispatch` 一个 `TriggerRequest`（`trigger_source = Cron`，幂等键 `cron:{id}:{scheduled_ms}`），再 `mark_ran` 推进 `next_run_at_ms`。`enabledSkillIds` 取 `user_settings.json` 全局启用列表（与 UI 技能库一致）。
- cron 表达式用 `cron` crate（`Schedule::from_str`，6 字段含秒）。
- **时区约定**：cron 字段（时/日/月…）按**用户本地时区**解释——`next_run_ms` / `mark_ran` / `set_enabled` / `next_run_ms_now` 一律传入 `chrono::Local::now()`（`cron::Schedule::after` 用 `after.timezone()` 解释字段，故 "9 点" = 本地 9 点）。所有时间戳列（`next_run_at_ms` / `last_run_at_ms` / `created_at_ms`）存的是**与时区无关的 UTC 毫秒瞬时**；前端用 `new Date(ms).toLocaleString()` 渲染为本地时间。新增任何「按字段解释 cron」的入口都必须传 `Local`，不要传 `Utc`。
- 启用策略：**server（web）与 desktop（Tauri）均默认开**（`POINTER_SCHEDULER_ENABLED=0` 可关）。
- 管理 API：`GET/POST /api/cron-jobs`、`PATCH/DELETE /api/cron-jobs/:id`；桌面端经 Tauri 命令 `list_cron_jobs / create_cron_job / update_cron_job / delete_cron_job` 同形操作。

### 专属 cron 会话（对齐 openclaw）

每个 cron 任务独占一个**隔离的专属会话**，不绑定用户的任何聊天会话：

- 会话 id 派生自任务 id：`cron_jobs::cron_session_id(job_id) = "cron:{job_id}"` 是稳定的会话**键**（存于 `cron_jobs.conversation_id`，用于索引/归类）；真正承载 transcript 的**活动会话 id** 是 `current_cron_session_id(job_id, now) = "cron:{job_id}:{yyyymmdd}"`（见下节重置策略）。`cron_jobs::insert` 忽略调用方传入的 `conversation_id`，强制写入 `cron:{job_id}`，杜绝把定时触发污染进用户会话。
- 首次触发时，scheduler 通过 `ConversationStore::ensure_cron_session(id, label)` 懒创建当前活动会话的 meta 行（`conversations` 表）。
- **跨次续接上下文**：`Scheduler::dispatch_job` 在每次触发时 `load_messages(当前活动会话 id)` 取回当日历史 transcript，与新提示词拼接后整体交给 `run_chat`；`run_chat` 的历史压缩会随轮次增长自动裁剪。
- **侧栏隔离**：`persist::load_all_from_conn` 与 `load_metas_from_conn` 都用 `WHERE id NOT LIKE 'cron:%'` 排除所有 cron 会话（含历史日期的旧会话），使其不出现在用户会话列表 / 分页 / 搜索；前端 `AppShell` 的 `filteredConversations` 额外做 `!id.startsWith('cron:')` 兜底过滤。
- **查看入口**：cron 会话不在侧栏列出，仅能从「自动化」面板任务行的「查看会话」按钮进入——`chat.openCronConversation(job.currentSessionId, label, leadAgentId, agentMode)` 用当前活动会话 id 构造临时 shell 注入 `conversations` 并 `selectConversation`，真实消息由 `ensureMessagesLoaded → loadConversationMessages` 按需水合；任务首次触发前 `currentSessionId` 为空，按钮禁用。

### Cron 会话重置策略（对齐 openclaw reset policy）

openclaw 的 cron 会话用 `daily` 重置模式、`atHour = 4`（本地凌晨 4 点）：会话的 `sessionStartedAt` 早于「最近一次 04:00 本地」即视为过期，过期后开新会话，且只继承偏好字段、**不继承 transcript**；旧 sessionId 的 transcript 文件不删，留在磁盘（openclaw 用「每 sessionId 一个 JSONL 文件」的存储模型，翻页 = 换新 uuid = 写新文件，旧文件留存）。我们已对齐该行为，并采用「换 id 留存」版而非「清空」版：

- 会话 id 编码重置日：`cron_jobs::current_cron_session_id(job_id, now) = "cron:{job_id}:{yyyymmdd}"`，其中 `yyyymmdd` 是「最近一次 04:00 本地边界」的日历日期（`daily_reset_at_ms(now, CRON_SESSION_RESET_AT_HOUR)`，`CRON_SESSION_RESET_AT_HOUR = 4`）。同一 `[D 04:00, D+1 04:00)` 窗口内的所有触发共享同一个 id → 续接同一段 transcript。
- `cron_jobs` 新增 `current_session_id TEXT` 列（schema v10 迁移），记录当前活动会话 id；首次触发前为 `NULL`，由 scheduler 懒设置。
- `Scheduler::dispatch_job` 每次触发：计算 `expected = current_cron_session_id(job.id, now_local)`；若 `job.current_session_id != expected`（首次触发或跨过 04:00），UPDATE `current_session_id = expected`、`ensure_cron_session(expected, label)` 建新会话 meta 行，本次提示词成为新 transcript 的首轮；否则 `load_messages(expected)` 续接当日历史。**不再 `clear_messages`**——翻页靠换 id 实现，旧 id 的 messages 原样留在 `messages` 表（对齐 openclaw 的留存）。
- 任务偏好（agent / prompt / cron 表达式）存在 `cron_jobs` 行上，天然跨翻页保留——对应 openclaw `sanitizeFreshCronSessionEntry` 只继承偏好字段。
- **侧栏隔离 + 查看入口**：所有 `cron:*` 会话（含历史日期的旧会话）都被 `load_all` / `load_metas` 的 `NOT LIKE 'cron:%'` 排除在侧栏外；前端 `AppShell.filteredConversations` 兜底过滤。「查看会话」按钮打开的是 `job.currentSessionId`（当前活动会话），任务首次触发前该字段为 `NULL`，按钮禁用并提示「尚未触发」。
- **对话内创建**：`general` agent 可通过 **`cron_job`** 工具在聊天中创建/列出/启停/删除定时任务；最小参数为 `prompt_text` + `schedule`（友好 preset 或 6 段 cron），其余字段（label、agent、job id）由工具自动填充。与设置页 Automation 面板写入同一张 `cron_jobs` 表。
- **与 openclaw 的一致与差异**：一致点——翻页换会话 id、旧 transcript 留存、应用层默认不暴露历史、偏好跨翻页保留。差异点——openclaw 用 uuid + 文件名留存（要靠列目录找回旧记录），我们用 `cron:{job_id}:{yyyymmdd}` 可读 id + `messages` 表留存（按 id 直接可查；后续如需 UI 历史回看，可在 cron 任务详情里列出该 job 的所有 `cron:{job_id}:*` 会话）。重置时刻目前为常量 4 点本地；如需可配置，后续在 `server_config` 增加 `scheduler.cron_session_reset_at_hour` 字段传入 `daily_reset_at_ms`。

## 钩子（HookRegistry）

8 个生命周期点：`on_trigger_received`（rewrite/reject）、`pre_dispatch`（reject）、`on_run_started`、`on_run_finished`、`on_run_failed`、`on_run_cancelled`、`pre_tool_call`、`post_tool_call`。注册顺序执行；observation 钩子出错只记 warn 不中断。内置 `LifecycleLogHook` 记录每次状态迁移。`pre/post_tool_call` 发射点暂未接入 `agent_tool_pass`（无消费者、高风险，记为后续）。

## 已知范围与后续

- **IM 入站**仍直连 `run_chat`（`pointer-channels/src/dispatch.rs`），未走 dispatcher。IM 已是事件驱动路径，且其 reply 收集依赖直接消费 `StreamEvent` 流；改走 dispatcher 需重写为消费 `AgentEvent`，收益低、回归风险高，暂缓。`enabledSkillIds` 取 `user_settings.json` 全局启用列表。
- **内部后台任务**（curator LLM pass、memory review）是定制 LLM 调用，不走 `run_chat`，与 dispatcher 的会话回合契约不匹配，故未迁移；`dispatch_internal` 供未来「会话回合型」内部触发使用。
- `pre/post_tool_call` 发射点未接入。
