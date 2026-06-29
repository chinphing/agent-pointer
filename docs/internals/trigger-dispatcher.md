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
| `Webhook` | `POST /api/webhooks/:src`（Bearer） | `server/src/main.rs::webhook_ingress` |
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

### 入口与鉴权

- **URL**：`POST /api/webhooks/:src`（仅 server/web 端；桌面 Tauri 无 HTTP ingress）
- **`:src`**：来源标识（字母/数字开头，仅 `-`、`_`），如 `github`、`codeup`
- **鉴权头**（默认）：`Authorization: Bearer <token>` 或 `X-Pointer-Token: <token>`（不用 query string）
- **自定义鉴权头**：每个来源可在自动化面板配置 Header 名（如 `X-Codeup-Token`）；配置后只读该 Header 的原始值
- **Token 存储**：`app_secrets` 标签 `webhook_token:{src}`，加密 at rest；UI 添加时 first-write-only
- **解析顺序**：`webhook_token:{src}` → 旧版全局 `webhook_bearer_token` → env `POINTER_WEBHOOK_BEARER_TOKEN`；皆无则 401
- **Body 上限**：256 KiB（含 raw-body 回退路径）

### 消息解析（两档，无 mapping 配置）

实现：`crates/pointer-core/src/webhook_ingress.rs` → `parse_webhook_body` / `build_webhook_dispatch_messages`。

**1. 结构化（默认优先）** — 当 body 含以下任一非空字段：

| 字段 | 说明 |
|------|------|
| `text` | Pointer 简写 |
| `message` | OpenClaw `/hooks/agent` 同名 |
| `messages` | 消息数组；仅 1 条 user 时 append 到 session；含 assistant/tool 或多条时视为完整历史 override |

可选控制字段见下表（均可与 raw-body 回退共存；第三方 JSON 里通常只带消息相关字段，控制字段由 Pointer 格式请求显式传入）：

**2. Raw body 回退** — 当 JSON 合法但无上述结构化消息（或 `text`/`message` 为空、`messages` 为空数组）：

- JSON → 整段 compact JSON 字符串作为 user 消息
- 非 JSON 纯文本 → 原文作为 user 消息
- 回退时 `name` 默认用 `:src`（如 `[github] {"ref":…}`），便于 GitHub/Codeup 等第三方原生 payload 零配置接入

Malformed JSON → **400**；空 body → **422**；超限 → **413**。

#### 可选 body 字段（`WebhookIngressBody`）

与 `text` / `message` / `messages` 不同，下表字段控制 **dispatch / HTTP 行为**，不参与 raw-body 回退判定（仍从 JSON 解析）。

| 字段 | 默认 | 作用 |
|------|------|------|
| `name` | 无 | 为本轮 user 消息加前缀 `[Name] …`；raw-body 回退且未传时改用 `:src`（如 `[github]`）。不改 session 标题。 |
| `conversationId` | 无 | 强制写入指定会话 id；缺省为 `webhook:{src}:{yyyymmdd}`（04:00 日切）。一般第三方 webhook 勿传。 |
| `agentMode` | 全局设置 | 本次 run 模式（如 `single` / `supervisor`），传入 `run_chat`。 |
| `leadAgentId` | 全局设置 | 本次主 Agent id（如 `general`），传入 `run_chat`。 |
| `idempotencyKey` | 无 | 幂等键；`runs` 表命中则返回已有 `runId`（`reused`），不重复执行。适合 GitHub 重试 delivery。 |
| `enabledSkillIds` | `[]` | 本次启用的 Skill id 列表。Webhook 仍按 body 传入；**IM / Cron** 未传时使用 `user_settings.json` 的 `enabledSkillIds`。 |
| `workspaceRoot` | 空 | 本次工作区根路径；空则按 session 默认 workspace 解析（与聊天一致）。 |
| `blocking` | `false` | `true` 时 HTTP 同步等待 run 结束并返回 assistant 文本；`false` 时 202 异步 ack。 |
| `timeoutSeconds` | `120` | 仅 `blocking: true` 有效；等待上限（秒），最大 600；超时 504。 |
| `attachments` | 无 | 与 `text` / `message` 同轮 user 消息的附件列表（`MediaAttachment`）。**推荐**引用 upload 返回的 `storageRelPath`；仅极小文件可用 `contentBase64`（见下「大小限制」）。 |

实现类型：`crates/pointer-core/src/webhook_ingress.rs`（`WebhookIngressBody`）；ingress 接线：`server/src/main.rs::webhook_ingress`。

### 附件上传（multipart）

#### 大小限制（三档，勿混用）

| 通道 | 上限 | 说明 |
|------|------|------|
| `POST /api/webhooks/:src` **整包 JSON** | **256 KiB** | 含 `text`、`attachments` 等所有字段；**最先**触达的上限 |
| 同上 JSON 内 `contentBase64`（解码后） | 6 MiB | 代码层单附件校验；在 256 KiB 整包限制下**实际达不到** |
| `POST /api/webhooks/:src/upload` **multipart** | **30 MiB** | 与 IM 入站一致；大文件**必须**走此路径 |

**实践建议**：inline `contentBase64` 只适合百 KiB 级小文件（Base64 膨胀约 +33%，还要扣 JSON 字段开销）。约 **≥ 200 KiB** 或需稳定传文件时，一律 **先 upload、再 JSON 引用 `storageRelPath`**。

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/webhooks/:src/upload` | 鉴权同 ingress；`multipart/form-data`：`file`（必填）、`fileName`（可选，亦可取自 part 文件名）、`mimeType`（可选）、`conversationId`（可选，默认当日 webhook session） |
| POST | `/api/webhooks/:src` | JSON：`text` + `attachments[]` 引用 upload 返回的 `storageRelPath` |

upload 响应示例：

```json
{
  "conversationId": "webhook:ci:20260629",
  "attachmentId": "wh-…",
  "storageRelPath": "webhook_ci_20260629/wh-…_report.pdf",
  "kind": "document",
  "mimeType": "application/pdf",
  "fileName": "report.pdf",
  "sizeBytes": 12345
}
```

典型流程：

```bash
# 1. 上传
curl -X POST "https://host/api/webhooks/ci/upload" \
  -H "Authorization: Bearer $TOKEN" \
  -F "file=@./report.pdf"

# 2. 触发 Agent（引用 storageRelPath）
curl -X POST "https://host/api/webhooks/ci" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"text":"请分析附件","attachments":[{"id":"wh-…","kind":"document","mimeType":"application/pdf","fileName":"report.pdf","storageRelPath":"webhook_ci_20260629/wh-…_report.pdf"}]}'
```

`storageRelPath` 必须属于当前 webhook 会话目录。

实现：`crates/pointer-core/src/webhook_attachment.rs`；HTTP：`server/src/main.rs::webhook_upload`。

### 会话与 dispatch

- **`conversationId` 缺省**：`resolve_webhook_ingress_session(:src)` → `webhook:{src}:{yyyymmdd}`（本地 **04:00** 日切，与 cron 相同）
- **Append 模式**：`load_messages` + 追加本轮 user → 同一天内续接 transcript
- **Trigger**：`TriggerRequest { trigger_source: Webhook, trigger_meta.webhook_source: src }` → `RunDispatcher::dispatch`
- **UI 同步**：广播 `InjectedUserMessage`，自动化面板「查看会话」可见 user 行
- **侧栏隔离**：`webhook:*` 不出现在用户会话列表；仅从自动化面板进入

### 响应模式

| 模式 | 行为 |
|------|------|
| 默认（异步） | **202** + `{ runId, status: accepted }` |
| `blocking: true` | 保持连接至 run 结束；成功 **200** `{ ok, runId, conversationId, text }`；失败 **500**；超时 **504**（`timeoutSeconds` 默认 120，最大 600） |

### 管理 API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/webhooks/config` | 列出已配置来源 + URL 模板 |
| POST | `/api/webhooks/config` | 添加来源 Token（+ 可选 `authHeaderName`） |
| DELETE | `/api/webhooks/config/:src` | 清除来源 |
| DELETE | `/api/webhooks/config/legacy` | 清除旧版全局 Token |

桌面端同形 Tauri 命令管理 Token；ingress 仅 web server。

### GitHub 示例

Repository Webhook → Payload URL `https://host/api/webhooks/github`，Secret 填 Pointer 为该来源生成的 Token（GitHub 发 HMAC 签名，Pointer 当前只验 Bearer/自定义 Header；Secret 需与 Token 一致并在 GitHub 侧重试，或中间层转发并加 `Authorization: Bearer`）。Push 原生 JSON 无 `message` 字段 → 自动 raw-body 回退，Agent 收到 `[github] {"ref":"refs/heads/main",…}`。

### 与 OpenClaw 差异（刻意简化）

- 无 `hooks.mappings` / JS transform；第三方 payload 靠 raw-body 回退 + Agent 自行理解
- 默认 session 按 `:src` 日切续接，非 isolated 单次
- 无 `deliver` 到 IM channel（`DeliverTarget::None`）

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
