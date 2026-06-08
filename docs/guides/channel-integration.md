# IM 通道对接指南

Pointer 通过 `pointer-channels` crate 以 **长连接优先、纯 Rust** 方式对接飞书、钉钉、企微、微信。

## 架构

- 入站（默认）：飞书 WSS、钉钉 Stream、企微 Bot WSS、微信 iLink 长轮询
- 入站（备选）：飞书 / 钉钉 / 企微 HTTP → `POST /webhooks/{channel}/{account_id}`
- 出站：各平台 Open API / sessionWebhook 主动推送
- 编排：入站消息 → `run_chat` → 回复文本回推

## 部署

桌面端（Tauri）保存配置并启用后，会自动启动各通道 monitor，**通常无需公网地址**。

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
2. 在弹出窗口中用**企业微信 App** 扫码，点击「一键创建智能机器人」
3. 成功后 `Bot ID` / `Secret` 自动填入，连接模式保持 **WSS 长连接**，启用并保存

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

## 配对

DM 策略默认为 `pairing`。未知用户会收到配对码，管理员在设置中输入配对码批准。

## 配置存储

- 通道配置：`{data_dir}/PointerApp/channels_config.json`
- 凭证：`{data_dir}/PointerApp/channel_credentials/`（加密）
- 会话历史：`{data_dir}/PointerApp/channel_histories/`
