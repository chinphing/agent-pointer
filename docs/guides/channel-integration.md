# IM 通道对接指南

Pointer 通过 `pointer-channels` crate 以 **长连接优先、纯 Rust** 方式对接飞书、钉钉、企微、微信。

## 架构

- 入站（默认）：飞书 WSS、钉钉 Stream、企微 Bot WSS、微信 iLink 长轮询
- 入站（备选）：飞书 / 钉钉 / 企微 HTTP → `POST /webhooks/{channel}/{account_id}`
- 出站：各平台 Open API / sessionWebhook 主动推送（文本 + 图片/文件）
- 编排：入站消息 → `run_chat` → 解析回复中的 `MEDIA:` 标记 → 文本与媒体分别回推

## 部署

桌面端（Tauri）**保存**仅写入通道配置，不重启 monitor；点击 **连接** 才会保存并重启对应长连接。启用通道后需点连接（或扫码流程结束后的自动连接）才会建立 WSS/Stream，**通常无需公网地址**。

仅当某通道选择 **Webhook 模式** 时：

1. 公网部署 `pointer-server`，或在开发环境用 ngrok 暴露端口
2. 在设置 → **IM 通道** 填写 `publicBaseUrl`
3. 将生成的 Webhook URL 填入各平台管理后台

## 飞书（WSS 长连接 · 推荐）

### 扫码一键创建（推荐）

1. 设置 → IM 通道 → 飞书 → 点击 **扫码一键创建**
2. 用**飞书 App** 扫描二维码，按提示完成应用授权
3. 成功后 `App ID` / `App Secret` 自动填入，连接模式保持 **WSS 长连接**，启用并保存

### 手动配置

1. 创建企业自建应用，开通消息收发权限
2. 事件订阅 → 选择 **使用长连接接收事件**
3. 订阅 `im.message.receive_v1`
4. Pointer 填写 `appId`、`appSecret`，连接模式选 **WSS 长连接**，启用并保存
5. 先启动 Pointer（建立长连接），再在飞书后台保存事件订阅配置
6. 日志：`feishu ws connecting`、`channel inbound channel=feishu`

### 飞书 Webhook 备选

回调地址：`{publicBaseUrl}/webhooks/feishu/default`，需填写 `encryptKey`。

## 钉钉（Stream 长连接 · 推荐）

### 扫码一键创建（推荐）

1. 设置 → IM 通道 → 钉钉 → 点击 **扫码一键创建**
2. 用**钉钉 App** 扫描二维码，点击「一键创建新机器人」
3. 成功后 `Client ID` / `Client Secret` 自动填入，连接模式保持 **Stream 长连接**，启用并保存

### 手动配置

1. 创建企业内部应用机器人，消息接收选 **Stream 模式**
2. Pointer 填写 `clientId`（AppKey）、`clientSecret`，连接模式选 **Stream 长连接**，启用并保存
3. 群聊需 @ 机器人；单聊直接发消息
4. 日志：`dingtalk stream connected`、`channel inbound channel=dingtalk`

### 钉钉 HTTP 备选

回调 URL：`{publicBaseUrl}/webhooks/dingtalk/default`

## 企微

### WSS 长连接（推荐 · 智能机器人 Bot）

#### 扫码一键创建（推荐）

1. 设置 → IM 通道 → 企微 → 点击 **扫码一键创建**
2. 用**企业微信 App** 扫描页内二维码，点击「一键创建智能机器人」
3. 成功后 `Bot ID` / `Secret` 自动填入，连接模式保持 **WSS 长连接**，启用并保存

与飞书/钉钉相同：后端请求企微 `/ai/qc/gen` 获取授权链接并生成二维码，轮询 `/ai/qc/query_result` 直至返回 `botId` / `secret`。

#### 手动配置

1. 企微管理后台创建**智能机器人**，获取 `botId` + `secret`
2. 设置 → IM 通道 → 企微 → 选择 **WSS 长连接**
3. 填写 `botId`、`secret`（可选改 `websocketUrl`，默认 `wss://openws.work.weixin.qq.com`）
4. 保存后重启应用 / `pointer-server`（monitor 在启动时连接）
5. **无需公网 Webhook**，桌面端 Tauri 也可直接连 WSS

### HTTP 回调（自建应用 Agent）

1. 自建应用 → 接收消息 → 设置回调 URL
2. URL：`{publicBaseUrl}/webhooks/wecom/default`
3. 配置 `corpId`、`agentId`、`secret`、`token`、`encodingAesKey`

## 微信

1. 在设置中点击「扫码登录」
2. 使用**微信 App** 扫描（二维码内容为 `qrcode_img_content` 返回的 liteapp 链接，非轮询令牌）
3. 手机端确认后凭证加密存储于本地 `channel_credentials/`
4. 勾选「启用」并保存，桌面端自动启动 iLink `getupdates` 长轮询 monitor
5. 首次私聊默认需配对（`dmPolicy: pairing`），或在设置里批准配对码

## 出站媒体（图片 / 文件）

四个 IM 通道均支持 Agent 向用户发送图片或文件。模型在回复末尾附加媒体路径行（对齐 OpenClaw `MEDIA:` 约定），dispatch 会自动解析并上传发送；这些行**不会**展示给用户。

支持的写法：

```
这是分析结果。
MEDIA:pointer-media://{convId}/{attachmentId}.png
MEDIA:/absolute/path/to/report.pdf
```

路径解析顺序：

1. `pointer-media://` 或 `conversation-media/` 相对路径 → 应用数据目录下的已保存附件
2. 绝对路径 → 直接读取
3. 相对路径 → 依次尝试数据目录、工作区

发送行为：

| 通道 | 文本 | 图片 | 文件 |
| --- | --- | --- | --- |
| 飞书 | post markdown | `im/v1/images` + image 消息 | `im/v1/files` + file 消息 |
| 钉钉 | sessionWebhook markdown | media/upload + image | media/upload + file |
| 企微 WSS | 流式 / markdown | WS 分片上传 + image/file 消息 | 同上 |
| 企微 Agent HTTP | text | media/upload + message/send | 同上 |
| 微信 iLink | text item | CDN 加密上传 + image_item | CDN 加密上传 + file_item |

单文件上限 30 MB。若路径无法解析或上传失败，会记录错误日志，文本回复仍会发送。

### `channel_message` 工具（对齐 OpenClaw `message` 工具）

IM 会话中 Agent 可调用 `channel_message` 主动发送，无需等最终回复：

```json
{ "action": "send", "text": "报告如下", "mediaUrls": ["/path/to/report.pdf"] }
```

### 路径白名单 `mediaLocalRoots`

设置 → IM 通道 → **出站媒体路径** 可配置额外允许目录（对齐 OpenClaw `mediaLocalRoots`）。
始终允许：`pointer-media://…` 与 `conversation-media/` 下已保存附件。

### 手动发送 API（对齐 `openclaw message send --media`）

```bash
curl -X POST http://127.0.0.1:8787/api/channels/feishu/default/send \
  -H 'Content-Type: application/json' \
  -d '{"recipientId":"ou_xxx","text":"hi","mediaUrls":["/path/file.png"]}'
```

钉钉需附带 `sessionWebhook`；微信需 `contextToken`；企微 Bot 被动回复可带 `wecomReqId`。

## 配对

DM 策略默认为 `pairing`。未知用户会收到配对码，管理员在设置中输入配对码批准。

## 会话重置

IM 通道的会话历史与 App 内「新会话」**相互独立**。可通过聊天指令或空闲超时开始新对话。

### 停止当前任务

对齐 OpenClaw `/stop` **快速中止路径**：在任务执行中发送以下指令会**立即**取消当前 `run_chat`（不排队等待），并回复「已停止当前任务。」：

- `/stop`、`/cancel`、`/abort`
- `停止`、`停下来`、`暂停`

同一 IM 会话同时只跑一个任务（新消息会等当前任务结束；停止后可立即发新消息）。

### 手动重置

在 IM 中发送以下任一内容即可清空当前会话历史（旧记录会归档，不会直接删除）：

- `/new`、`/reset`
- `新对话`、`重新开始`

仅发送重置指令时，机器人回复「已开始新对话。」且**不会**把该指令交给模型。  
若附带后续文字（如 `/new 帮我查天气`），会先重置再处理后续内容。

手动重置**不会**从渠道 API 回填历史消息。

### 空闲自动重置

所有 IM 通道共用一项全局配置（`channels_config.json` → `meta`）：

```json
"meta": {
  "sessionReset": { "idleMinutes": 60 }
}
```

默认 **60 分钟**（1 小时）无新消息后，下一条入站消息会从空历史开始；旧记录同样归档到 `channel_histories/archives/`。  
`idleMinutes` 设为 `0` 可关闭。设置页 → IM 通道 → **会话重置** 可修改（对所有通道生效）。

## 配置存储

- 通道配置：`{data_dir}/PointerApp/channels_config.json`
- 凭证：`{data_dir}/PointerApp/channel_credentials/`（加密）
- 会话历史：`{data_dir}/PointerApp/channel_histories/`
- 会话元数据：`{data_dir}/PointerApp/channel_histories/*_meta.json`（`lastInteractionAt`）
- 归档历史：`{data_dir}/PointerApp/channel_histories/archives/`
