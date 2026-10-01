# Webhook API

通过 HTTP 触发 Pointer Agent（CI、GitHub、自定义系统等）。**仅 Pointer server** 提供入站；需先部署并暴露 `pointer-server` 公网地址。

> IM 平台回调（飞书/钉钉/企微 HTTP 入站）见 [channel-integration.md](channel-integration.md)，与本 API 不同。

## 快速开始

1. 在 Pointer **设置 → 自动化** 添加来源（如 `ci`），获取 **Token** 与完整 URL。
2. 向 `POST https://<host>/api/webhooks/<src>` 发送请求，Header 带 Token。
3. 默认 **202** 异步执行；需要同步结果时设 `blocking: true`。

## 鉴权

| 项 | 说明 |
|----|------|
| URL | `POST /api/webhooks/:src` |
| `:src` | 来源 id（字母/数字开头，仅 `-`、`_`），须与自动化面板中配置一致 |
| Token | 面板生成；请求头二选一：`Authorization: Bearer <token>` 或 `X-Pointer-Token: <token>` |
| 自定义头 | 面板可为来源指定专用 Header 名（如 `X-My-Token`），此时只认该头的原始值 |

勿在 query string 传 Token。鉴权失败 → **401**。

## 设置 API（管理 Token）

自动化面板通过以下接口管理来源（需 **账户登录**，与 Webhook 入站 Bearer 鉴权不同）：

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/webhooks/config` | 列出已配置来源（仅尾号 preview，不含完整 Token） |
| POST | `/api/webhooks/config` | 添加来源 Token（仅首次写入）；可选 `sessionMode` |
| PATCH | `/api/webhooks/config/:src` | 更新来源设置（如 `sessionMode`） |
| GET | `/api/webhooks/config/:src/token` | 查看完整 Token（供设置页复制） |
| DELETE | `/api/webhooks/config/:src` | 删除来源 Token |
| DELETE | `/api/webhooks/config/legacy` | 清除旧版全局 Token |

**Reveal 响应示例**：

```json
{
  "src": "ci",
  "token": "abc123…",
  "preview": "****1234"
}
```

## 触发 Agent

### 请求体

**消息字段**（至少一种；均无则走 [原生 payload](#原生-payload-回退)）：

| 字段 | 说明 |
|------|------|
| `text` | 发给 Agent 的文本 |
| `message` | 同 `text`（兼容别名） |
| `messages` | 消息数组；单条 `user` 时追加到当日会话；含 `assistant`/`tool` 或多条时视为完整历史 |

追加（单条 `user`，或 `text` / `message`）只提交这一句。
服务端把它写入当日会话，再按工作集开跑，不把库里的整段历史读进这次请求。
完整历史直接使用请求里的 `messages`，不先读库。

**可选控制字段**：

| 字段 | 默认 | 说明 |
|------|------|------|
| `name` | 无 | 为本轮消息加前缀 `[name] …` |
| `conversationId` | 自动 | 一般勿传；缺省按来源 + 本地日切续接会话 |
| `agentMode` | 实例默认 | 如 `single` |
| `leadAgentId` | 实例默认 | 如 `general`、`coder` |
| `idempotencyKey` | 无 | 幂等键；重复请求返回同一 `runId`，不重复执行 |
| `enabledSkillIds` | `[]` | 本次启用的 Skill id 列表 |
| `workspaceRoot` | 空 | 工作区绝对路径；空则用会话默认 |
| `blocking` | `false` | `true` 时同步等待 Agent 结束 |
| `timeoutSeconds` | `120` | `blocking` 时最长等待秒数，最大 600 |
| `attachments` | 无 | 附件列表，见 [附件](#附件) |
| `deliver` | 无 | Run → IM 投递规格（如 `feishu`、`feishu:ou_xxx`、`feishu:group:oc_xxx`、`all`）；run 结束后把最终回复推送到对应通道。格式见 [channel-integration.md](channel-integration.md) |

整包 JSON 上限 **256 KiB**。空 body → **422**；非法 JSON → **400**；超限 → **413**。

### 原生 payload 回退

未提供 `text` / `message` / `messages`（或均为空）时：

- **JSON body** → 整段 compact JSON 作为 user 消息（前缀 `[src]`，如 `[github] {"ref":…}`）
- **纯文本 body** → 原文作为 user 消息

适合 GitHub、Codeup 等直接 POST 原生 webhook body，无需改格式。

### 响应

| 模式 | HTTP | 响应体 |
|------|------|--------|
| 异步（默认） | **202** | `{ "runId": "…", "status": "accepted" }` |
| 异步轮询 | **GET** `/api/webhooks/:src/runs/:runId` | `{ "runId", "status", "conversationId", "text?", "error?" }` |
| 同步 `blocking: true` | **200** | `{ "ok": true, "runId": "…", "conversationId": "…", "text": "…" }` |
| 同步失败 | **500** | 错误信息 |
| 同步超时 | **504** | 超过 `timeoutSeconds` |

幂等命中时异步仍返回 **202**，`status` 可能为 `reused`。

### 异步轮询结果

默认 **202** 仅表示 run 已入队。可用同一来源 Token 轮询：

```http
GET /api/webhooks/:src/runs/:runId
Authorization: Bearer <token>
```

| `status` | 说明 |
|----------|------|
| `queued` / `running` | 仍在执行；`text` / `error` 为空 |
| `finished` | 完成；`text` 为 Agent 最终回复 |
| `failed` / `cancelled` | 终止；`error` 含原因 |

建议间隔 1–3 秒轮询，直至 `status` 为终态。

### 会话续接

未传 `conversationId` 时，会话分组由来源的 **`sessionMode`** 决定（自动化面板可配置）：

| 模式 | 说明 |
|------|------|
| `per_delivery`（默认） | 每次投递独立会话；delivery id 取自 `idempotencyKey` → `X-GitHub-Delivery` / `Svix-Id` / `X-Request-ID` → 自动生成 |
| `daily` | 同一 `:src` 在**本地日历日**（04:00 切换）内共享上下文 |

`per_delivery` 下建议将 GitHub `delivery` id 写入 `idempotencyKey`，避免重试开新会话。

## 附件

大文件请 **先上传、再引用**；勿把大文件塞进 JSON。

| 步骤 | 方法 | 路径 | 上限 |
|------|------|------|------|
| 上传 | POST | `/api/webhooks/:src/upload` | **30 MiB**（multipart） |
| 触发 | POST | `/api/webhooks/:src` | JSON 引用 `storageRelPath` |

**Upload** — `multipart/form-data`：

| 字段 | 必填 | 说明 |
|------|------|------|
| `file` | 是 | 文件内容 |
| `fileName` | 否 | 文件名（亦可取自 part 文件名） |
| `mimeType` | 否 | MIME 类型 |
| `conversationId` | 否 | 与会话 id 一致时可指定；缺省为当日 webhook 会话 |

鉴权同触发接口。

**Upload 响应示例**：

```json
{
  "conversationId": "webhook:ci:20260629",
  "attachmentId": "wh-abc123",
  "storageRelPath": "webhook_ci_20260629/wh-abc123_report.pdf",
  "kind": "document",
  "mimeType": "application/pdf",
  "fileName": "report.pdf",
  "sizeBytes": 12345
}
```

**触发时引用** — `attachments` 数组元素示例：

```json
{
  "id": "wh-abc123",
  "kind": "document",
  "mimeType": "application/pdf",
  "fileName": "report.pdf",
  "storageRelPath": "webhook_ci_20260629/wh-abc123_report.pdf"
}
```

`storageRelPath` 须来自同一会话的 upload 响应。

### 示例（curl）

```bash
TOKEN="<面板中的 Token>"
HOST="https://your-pointer-host"

# 1. 上传
curl -X POST "$HOST/api/webhooks/ci/upload" \
  -H "Authorization: Bearer $TOKEN" \
  -F "file=@./report.pdf"

# 2. 触发（将 upload 返回的字段填入 attachments）
curl -X POST "$HOST/api/webhooks/ci" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "text": "请分析附件",
    "attachments": [{
      "id": "wh-abc123",
      "kind": "document",
      "mimeType": "application/pdf",
      "fileName": "report.pdf",
      "storageRelPath": "webhook_ci_20260629/wh-abc123_report.pdf"
    }]
  }'
```

## GitHub 对接示例

1. Pointer 自动化面板添加来源 `github`，复制 Token。
2. GitHub → Settings → Webhooks → Payload URL：`https://<host>/api/webhooks/github`
3. **Secret**：与 Pointer Token 相同；若 GitHub 只发 HMAC 签名，可在中间层校验后转发并加上 `Authorization: Bearer <token>`。
4. Push 等事件的 JSON 无 `message` 字段 → 自动走原生 payload 回退，Agent 收到 `[github] {"ref":"refs/heads/main",…}`。
5. 建议设 `idempotencyKey` 为 GitHub `delivery` id，避免重试重复执行。

## 同步调用示例

```bash
# 异步触发 + 轮询
RESP=$(curl -s -X POST "$HOST/api/webhooks/ci" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"text":"总结本次构建日志"}')
RUN_ID=$(echo "$RESP" | jq -r .runId)
until true; do
  BODY=$(curl -s "$HOST/api/webhooks/ci/runs/$RUN_ID" \
    -H "Authorization: Bearer $TOKEN")
  STATUS=$(echo "$BODY" | jq -r .status)
  case "$STATUS" in finished|failed|cancelled) echo "$BODY" | jq .; break ;; esac
  sleep 2
done
```

```bash
curl -X POST "$HOST/api/webhooks/ci" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "text": "总结本次构建日志",
    "blocking": true,
    "timeoutSeconds": 300
  }'
```

成功时响应 `text` 为 Agent 最终回复正文。

用户侧 Python 多附件示例见 **[`../user/webhook.md`](../user/webhook.md)**。
