# 平台登录刷新：网络抖动 vs 需要重新登录

## 问题

access token 过期后，发送消息会先 `refresh_if_needed()`。若此时官网换票接口因网络抖动失败：

1. 内存里仍有 session / `auth.dat` 仍有 refresh token（**并未真正登出**）
2. `session_view().logged_in` 因 access 过期变为 `false`
3. 旧逻辑把这种情况统一报成 **「请先登录 Pointer 账户」**

用户侧看起来像掉登录，实际多半是瞬时网络问题。

## 现行区分

| 情况 | 判定 | 用户提示 |
|------|------|----------|
| `http_status=401/403` / `invalid_refresh_token` | 真失效，清除本地 refresh | 登录已失效 / 请先登录 |
| `http_status=408/429/5xx`，或无状态码的传输错误 | 瞬时失败，**保留** `auth.dat`，退避重试 | 仍失败才提示网络异常 |
| 其他 `http_status=4xx` | 不重试、不按「掉登录」清会话 | 透传/业务错误文案 |

换票失败会带稳定标记：`token exchange failed http_status=502 (...)`，前端 `extractPlatformAuthHttpStatus` 优先读该字段，避免用正文里偶然出现的 `401` 误判。

## 统一登录门禁（前端 / 桌面）

需要「已登录」才能继续的动作，不要各自拼 `ensureFreshSession` + `logged_in` 判断。

| 层 | 入口 | 说明 |
|----|------|------|
| 前端动作 | `platformAuth.requireSession({ purpose, onTransient })` | 先刷新再校验；文案用 `loginHint` / `formatLoginGateError` |
| 桌面 IPC | `platform_auth_gate::require_logged_in` / `require_platform_user_id` | 先 `refresh_if_needed`，再查 `logged_in`（及 user id） |
| Web HTTP | `require_platform_access` | Cookie 侧不主动换票；浏览器由 `ensureFreshSession` → `/api/auth/refresh` 触发 |

`onTransient`：

- `error`（默认，发消息）：瞬时换票失败时抛出网络文案，不假装未登录
- `allow`（Composer 附件）：瞬时失败且 UI 仍显示已登录时放行，由后端再拦一次

## 附件上传与登录态

Composer / 发送前走 `requireSession`；桌面 `save_chat_attachment` 走 `require_platform_user_id`。  
否则 access token 过期时，界面仍可能显示已登录，上传却报 **「请先登录 Pointer 账户」**（UI 快照与 Rust 侧 `expires_at` 不同步）。

网页端 multipart 401 / `platform_login_required` 经 `formatLoginGateError(..., 'attachment')` 映射为同一登录提示。

## 瞬时失败重试

以下两处共用同一套退避（1 次立即 + 3 次重试：800ms → 2s → 4s）：

| 调用点 | 接口 | 仍失败时的提示 |
|--------|------|----------------|
| `refresh_if_needed` | `POST /auth/app/token` | 网络异常，暂时无法验证登录态，请稍后重试 |
| `fetch_partner_balance`（`ensure_llm_allowed`） | `GET /auth/partner/balance`（每轮对话前） | 网络异常，请检查网络链接是否正常，然后重试。 |

换票持 `refresh_lock`；鉴权失败（401/403 / `platform_token_expired`）**不重试**，立即按登录失效处理。余额耗尽（`token_quota_exhausted`）不重试。

实现要点：

- `PlatformAuth::is_refresh_auth_failure`：按 `http_status=` 判定，不用正文偶然出现的 `401`
- `session_inner`：refresh / balance 瞬时失败不走「请先登录」
- `ensure_access_token`：瞬时失败返回网络文案
- 前端 `platformAuth` / `sendUserMessage`：瞬时错误不把 UI 会话打成未登录
