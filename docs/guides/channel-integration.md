# IM 通道对接指南

Pointer 通过 `pointer-channels` crate 以 **Webhook 优先、纯 Rust** 方式对接飞书、钉钉、企微、微信（iLink 长轮询）。

## 架构

- 入站：飞书 / 钉钉 / 企微 → `POST /webhooks/{channel}/{account_id}`
- 入站：微信 → `getupdates` HTTP 长轮询（无 Webhook）
- 出站：各平台 Open API 主动推送
- 编排：入站消息 → `run_chat` → 回复文本回推

## 部署

1. 公网部署 `pointer-server`，或在开发环境用 ngrok 暴露端口
2. 在设置 → **IM 通道** 填写 `publicBaseUrl`（如 `https://pointer.example.com`）
3. 将生成的 Webhook URL 填入各平台管理后台

## 飞书

1. 创建企业自建应用，开通消息收发权限
2. 事件订阅地址：`{publicBaseUrl}/webhooks/feishu/default`
3. 配置 `appId`、`appSecret`、`encryptKey`
4. 订阅 `im.message.receive_v1`

## 钉钉

1. 创建企业内部应用机器人，消息接收选 **HTTP 模式**
2. 回调 URL：`{publicBaseUrl}/webhooks/dingtalk/default`
3. 配置 `clientId`（AppKey）、`clientSecret`

## 企微

### WSS 长连接（推荐 · 智能机器人 Bot）

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
2. 凭证加密存储于本地 `channel_credentials/`
3. 服务端自动启动 iLink 长轮询 monitor

## 配对

DM 策略默认为 `pairing`。未知用户会收到配对码，管理员在设置中输入配对码批准。

## 配置存储

- 通道配置：`{data_dir}/PointerApp/channels_config.json`
- 凭证：`{data_dir}/PointerApp/channel_credentials/`（加密）
- 会话历史：`{data_dir}/PointerApp/channel_histories/`
