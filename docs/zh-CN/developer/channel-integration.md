# IM 通道对接指南

> 终端用户在设置界面的操作摘要见 **[`../user/im-channels.md`](../user/im-channels.md)**。下文为各平台完整对接与 Webhook 部署说明。

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

### 群 @ 与动态 Agent（企微）

**群 @**：Bot WSS 模式下，企微平台仅在用户 @ 机器人时推送消息；Pointer 会剥离消息中的 `@机器人名` 前缀。Agent HTTP 回调模式下，通过文本是否以 `@` 开头判断；`requireMention: true` 时未 @ 的群消息会被丢弃。

**动态 Agent**（`dynamicAgents`，默认启用）：

| 字段 | 默认 | 说明 |
| --- | --- | --- |
| `enabled` | `true` | 启用动态会话路由 |
| `dmCreateAgent` | `true` | 私信按用户隔离会话 |
| `groupEnabled` | `true` | 群聊按群共享一个会话（非按发送者） |
| `adminUsers` | `[]` | 列表内 userid 走主会话 `_main`，不隔离 |

群聊共享会话时，入站消息会自动加 `[userid]:` 前缀，便于模型区分发言人。配置示例：

```json
{
  "wecom": {
    "default": {
      "dynamicAgents": {
        "enabled": true,
        "groupEnabled": true,
        "adminUsers": ["admin_userid"]
      }
    }
  }
}
```

## 微信

1. 在设置中点击「扫码登录」
2. 使用**微信 App** 扫描（二维码内容为 `qrcode_img_content` 返回的 liteapp 链接，非轮询令牌）
3. 手机端确认后凭证加密存储于本地 `channel_credentials/`
4. 勾选「启用」并保存，桌面端自动启动 iLink `getupdates` 长轮询 monitor
5. 首次私聊默认需配对（`dmPolicy: pairing`），或在设置里批准配对码

## IM 会话内切换智能体

在企微 / 飞书 / 钉钉 / 微信 IM 会话中，可直接发送 **智能体显示名称** 或 **id** 切换模式（无需固定口令）：

| 输入示例 | 效果 |
| --- | --- |
| `电脑操控` | 切换到电脑操控 |
| `通用助手` | 切换到通用助手 |
| `氛围编程` | 切换到氛围编程 |
| `电脑操控 帮我打开浏览器` | 先切换，再执行后续问题 |

IM 会话内仅暴露以上三个智能体；其他内置智能体（如深度研究）不可通过 IM 口令切换。

切换成功会回复：`已切换到…。`  
仍支持会话重置：`/new`、`/reset`、`新对话`、`重新开始`。

每个 IM 会话独立记住当前智能体（会话级 `leadAgentId`）；与桌面其他会话互不影响。IM 切换智能体时，桌面侧栏对应会话与 Composer 会同步更新。

氛围编程工作目录：在桌面为该会话选择项目文件夹（会话级 `workspaceRoot`）；未设置时使用默认会话沙箱（按 `session_user_id` 或匿名会话目录，见 [workspace-root.md](workspace-root.md)）。用户在 Composer 点 ✕ 清除后，改为使用默认沙箱；沙箱目录在**首次发送消息时**才创建，清除时不会在磁盘上立即建目录。

## 桌面端镜像 IM 对话

IM 入站触发 `run_chat` 时，流式事件（工具调用、推理、子 Agent 轨迹等）会广播到 Pointer 主界面，侧栏会出现对应 IM 会话，可实时查看中间过程与最终回复（与桌面聊天一致）。

## 出站媒体（图片 / 文件）

四个 IM 通道均支持 Agent 向用户发送图片或文件。模型在回复末尾附加媒体路径行（对齐 OpenClaw `MEDIA:` 约定），dispatch 会自动解析并上传发送；这些行**不会**展示给用户。

**图表：** 回复中的 Markdown ` ```chartjs ` / ` ```chart ` fence（Chart.js JSON）在出站前由主机规范化（与 App 相同的柔和色板 + 白底，去掉非法回调字符串）后，用**系统中文字体**栅格为 PNG（`generated-media/im-charts/`），并改写为 `MEDIA:` 附件发送（可用 `POINTER_IM_CHART_FONT` 覆盖字体路径）。多 Y 轴（`y1`/`y2`/…）：fulgur 不支持次轴，主机将各次轴系列按各自尺度线性映射到主轴以对齐曲线形态（图例标注 `y1 尺度…` 等；次轴刻度不绘制）。桌面/Web 仍为交互式图表。实现见 `chart_outbound.rs` / `im_chart_font.rs`；说明见 [`../ui/markdown-charts.md`](../ui/markdown-charts.md)。

支持的写法：

```
这是分析结果。
MEDIA:pointer-media://{convId}/{attachmentId}.png
MEDIA:pointer-media://generated-media/{user}/{conv}/{uuid}.png
MEDIA:/absolute/path/to/report.pdf
pointer-media://{user}/{convId}/{file}.md
```

裸 `pointer-media://…` 整行（无 `MEDIA:` 前缀）在**文件可解析**时同样作出站附件；解析失败则保留在可见正文，便于排查。

绝对路径可含空格（如 macOS `…/Library/Application Support/…`）。主机对同行写法（如 `- 文档：MEDIA:/…/Application Support/…/x.md`）按**整段路径**解析；旧逻辑用 `[^\s]+` 会在第一个空格处截断。整段不存在时，仅当空格后**不像路径续段**（如 `MEDIA:/tmp/a.png world`）才回退到第一个 token；若续段含 `/` 或 `\`（`Application` + `Support/…`），即使 `…/Library/Application` 这个 0 字节前缀文件存在，也不得当成附件。文件不存在则**不入附件**，且按约定**不得剔除正文中的 `MEDIA:`**（`5df5847` / `split_reply_media`：仅文件真实存在时才抽出）。**0 字节空文件也算有效文件**（`Path::is_file()`），不会因 size 为 0 拒绝。可选 `` ` `` / `"` / `'` 包裹路径；缺少闭合引号时回退为无引号路径解析。实现见 `media/outbound_reply.rs`。

路径解析顺序：

1. `pointer-media://` 或 `conversation-media/` 相对路径 → 应用数据目录下的已保存附件
2. `generated-media/`、`session-sandboxes/` 前缀 → `{app_data}/` 下对应子目录（AI 生成图/视频、会话沙箱产出）
3. 绝对路径 → 直接读取
4. 相对路径 → 依次尝试数据目录、工作区

出站解析（`resolve_outbound_media`）对 app 托管路径（storage rel、`generated-media/`、`session-sandboxes/`）在本地解析成功后即视为可投递；仅工作区相对路径额外要求落在 `app_data` 内，避免 server 模式读到本机 cwd 外文件。

发送行为：

| 通道 | 文本 | 图片 | 文件 |
| --- | --- | --- | --- |
| 飞书 | post markdown | `im/v1/images` + image 消息 | `im/v1/files` + file 消息 |
| 钉钉 | sessionWebhook markdown | media/upload + image | media/upload + file |
| 企微 WSS | 流式 / markdown | WS 分片上传 + image/file 消息 | 同上 |
| 企微 Agent HTTP | text | media/upload + message/send | 同上 |
| 微信 iLink | text item | CDN 加密上传 + image_item | CDN 加密上传 + file_item / video_item |

微信图片出站：`image_item.mid_size` / `hd_size` 必须填 **AES 密文长度**（不是 PNG 明文大小），`media.aes_key` 使用 `base64(hex key)`（与 OpenClaw/Hermes 一致）；填错会出现「消息已发出但客户端裂图」。图表 PNG 走同一上传路径。

微信 iLink 出站会缓存每条入站的 `context_token`（按账号 + 用户）。Agent 长跑时：缓存超过 **45 秒**会在发送前调用 `getconfig` 尝试刷新（先带旧 token，再不带 token）；若 `sendmessage` 返回 `ret=-2 errmsg=unknown`，会再刷新并重试一次（接受 `getconfig` 返回相同 token 的情况）。用户长时间未发消息且 refresh 仍失败时，需用户再发一条激活会话。

单文件上限：非视频 **30 MB**；**视频**与 Composer OSS 一致（**5 GB** 硬上限，**>500 MB** 自动压缩后上传）。若路径无法解析或上传失败，会记录错误日志，文本回复仍会发送。

**IM 大文件出站（全通道）：** 超过 IM 直传上限（**20 MB**，对齐企微 `40006`）时，不再尝试平台附件上传，改为签发 **HMAC 限时下载链接**（`GET /api/media/public-download?token=…`，无需登录），以 Markdown 链接文本发给用户（形如 `📎 [file.zip](https://…)`）。直传失败时也会回退到同一链接。需配置公网可达的 `POINTER_SERVER_PUBLIC_URL`（或 channels `publicBaseUrl`）。可选：`POINTER_MEDIA_DOWNLOAD_SECRET`、`POINTER_MEDIA_DOWNLOAD_TTL_SECS`（默认 7 天，最长 7 天）。单链接文件上限 **512 MB**。

### 入站自消息过滤（避免多跑一轮）

部分 IM 平台会把**机器人自己发出的消息**再推回应用，若不过滤会被当成用户新消息，触发多余的 `run_chat` 轮次（飞书典型表现：先发文字、再发文件 → 文字回显又触发一轮）。

| 通道 | 平台是否会回推自消息 | 本项目处理 |
| --- | --- | --- |
| 飞书 | 会（`sender_type` 为 `app` / `bot`） | `feishu/parse.rs` 丢弃自消息 |
| 微信 iLink | 一般不会（`message_type=2` 为 BOT） | 仅接受 `message_type=1`；额外丢弃 `*@im.bot` 发送方 |
| 企微 WSS | 不会（文档：`aibot_send_msg` 不触发回调） | 仅处理 `aibot_msg_callback` 用户消息 |
| 钉钉 Stream | 一般不会（topic 为用户 → 机器人） | `senderId == chatbotUserId` 时丢弃自消息 |

### 出站回复（对齐 OpenClaw）

IM 会话中模型将**最终正文写在 assistant 消息**里，宿主在 `run_chat` 结束后自动发到对应 IM 通道；无需独立 delivery 工具（本项目已移除早期的 `channel_message` 工具，避免与最终回复重复）。

**中间轮次推送**（设置 → IM 通道 → **IM 出站推送**）：

| 选项 | 默认 | 说明 |
|------|------|------|
| `sendIntermediateText` | 开启 | 每轮模型输出（`MessageEnd` 有正文）立即推送给 IM 客户 |
| `sendToolCalls` | 开启 | 仅推送工具**开始调用**（`🔧 标签`）；**不**推送完成/失败状态行 |

`ask_user` 澄清选项由宿主主动推送（Hermes 风格编号列表），不走工具状态行。

最终回复若与已推送的中间文字相同则不会重复发送；`MEDIA:` 附件按可解析的本地路径发送（`pointer-media://…`、绝对路径、相对路径均可，禁止 `..` 路径穿越）。

### 手动发送 API（对齐 `openclaw message send --media`）

```bash
curl -X POST http://127.0.0.1:8787/api/channels/feishu/default/send \
  -H 'Content-Type: application/json' \
  -d '{"recipientId":"ou_xxx","text":"hi","mediaUrls":["/path/file.png"]}'
```

钉钉需附带 `sessionWebhook`；微信需 `contextToken`；企微 Bot 被动回复可带 `wecomReqId`。

## 配对

DM 策略默认为 `pairing`。未知用户会收到配对码，管理员在设置中输入配对码批准。

桌面/网页端**不会**空闲常轮询 pending 接口：仅在签发配对码（流事件 `channel_pairing_pending`）或启动时发现仍有未过期 pending 时，才短时轮询（约 4s）；pending 清空或超过后端 TTL（5 分钟）后停止。

## 会话重置

IM 通道的模型上下文与 App 内桌面会话可通过聊天指令或空闲超时重置。重置后会在侧边栏**新建一条空白会话**（标题带「新对话」），旧会话记录仍保留在列表中可回看；新的 IM 消息进入新会话。

### 停止当前任务

对齐 OpenClaw `/stop` **快速中止路径**：在任务执行中发送以下指令会**立即**取消当前 `run_chat`（不排队等待），并回复「已停止当前任务。」：

- `/stop`、`/cancel`、`/abort`
- `停止`、`停下来`、`暂停`

同一 IM 会话同时只跑一个任务（新消息会等当前任务结束；停止后可立即发新消息）。

### `ask_user`（对齐 Hermes `clarify`）

IM 上 `ask_user` **同轮阻塞**（默认最多约 600 秒）：

1. 宿主在 `ask_user` 阻塞前主动推送 Hermes 风格澄清文案（问题 + 编号选项 + 回复提示）
2. 调用 `ask_user` 后等待用户下一条消息
3. `ChannelGateway::process_inbound` 在开启新 turn 前拦截：能映射则映射编号/标签，否则按 Hermes 把原文当作回答 → 填入 tool result
4. 同轮继续跑模型；仅空消息会提示并继续等待

`/stop` 会取消阻塞。超时则 tool 返回 `timed_out: true`，模型自行默认或再问。
App / Web 交互卡在给定选项外提供「其他」自由输入；后端与 IM 一样接受选项外原文。

### 手动重置

在 IM 中发送以下任一内容即可清空当前会话历史（旧记录会归档，不会直接删除）：

- `/new`、`/reset`
- `新对话`、`重新开始`

仅发送重置指令时，机器人回复「已开始新对话。」且**不会**把该指令交给模型；桌面端会切换到新的空白侧边栏会话。  
若附带后续文字（如 `/new 帮我查天气`），会先重置并 fork 新侧边栏会话，再处理后续内容。

手动重置**不会**从渠道 API 回填历史消息。

### 空闲自动重置

所有 IM 通道共用一项全局配置（`channels_config.json` → `meta`）：

```json
"meta": {
  "sessionReset": { "idleMinutes": 60 }
}
```

默认 **60 分钟**（1 小时）无新消息后，下一条入站消息会 fork 新的桌面会话（`@sN`）；旧 fork 仍保留在 `conversations.db` 中。  
`idleMinutes` 设为 `0` 可关闭。设置页 → IM 通道 → **会话重置** 可修改（对所有通道生效）。

## 配置存储

- 通道配置：`{data_dir}/PointerApp/channels_config.json`
- 凭证：`{data_dir}/PointerApp/channel_credentials/`（加密）
- 会话历史与 IM 元数据：`{data_dir}/PointerApp/conversations.db`（与桌面会话共用）
- IM 线程状态（`sessionEpoch`、`activeConversationId`、idle 用的 `updatedAt`）保存在 base 会话行的 `conversations` 表字段中
- 旧版 `channel_histories/` 首次启动时会改名为 `channel_histories.deprecated/`（仅尝试导入其中的 `activeConversationId` 元数据）

## Run → IM 出站总线

把任意 run 的最终回复（或运行中主动构造的消息）推送到 IM 通道。覆盖**静态**（触发时配置投递目标）与**动态**（agent 运行中决定推送）两条路径。框架住 `pointer-channels`，触发源侧的注入住 `pointer-core`。

### deliver 字符串格式

投递目标用一个字符串描述，存放在 `trigger_meta.extra.deliver`（持久化到 `runs.trigger_meta_json`）。格式：

| 写法 | 含义 |
|------|------|
| `feishu` | 飞书账号的 home channel（需在 `channels_config.json` 配 `homeRecipientId`） |
| `feishu:ou_xxx` | 飞书 DM（按 open_id） |
| `feishu:group:oc_xxx` | 飞书群（按 chat_id，`oc_` 前缀自动识别） |
| `dingtalk:userId` | 钉钉 DM（走 `oToMessages/batchSend`） |
| `dingtalk:group:openConversationId` | 钉钉群（走 `groupMessages/send`，openConversationId 会镜像到 `reply_context.chat_id`） |
| `wecom:userid` | 企微 DM（Agent HTTP `message/send`） |
| `wecom:group:chat_id` | 企微群（WSS `send_markdown(chat_id)`） |
| `weixin:wxid` | 微信 iLink DM（需该用户曾向 bot 发过消息，`context_token` 缓存命中才可推送） |
| `a,b,c` | 逗号分隔多目标 |
| `all` | 所有已配置 home channel 的通道 |

解析在 `pointer-channels/src/im_delivery.rs::resolve_delivery_targets`，各通道差异在解析层吸收，hook 与 `im_send` 工具只调 `ChannelGateway::send_outbound_explicit`。

### 静态路径（Cron）

1. `cron_jobs.deliver` 列存 deliver 字符串（`cron_job` 工具 / 设置页 Automation 面板创建时填写）。
2. `scheduler.rs dispatch_job` 把 `job.deliver` 注入 `TriggerRequest.trigger_meta.extra.deliver`，置 `DeliverTarget::Im` 标记位；用户消息经 `build_cron_user_prompt` 前缀 Hermes 式 `[IMPORTANT…]`（勿用 `im_send` / `[SILENT]`），**不**再注入 cron system 块。
3. run 结束 → `ImDeliverHook`（`OnRunFinishedHook`）读取 `trigger_meta.extra.deliver`，加载最后一条 assistant 回复，拆媒体 / 跳过静默叙述 / 截断到 4000 字，逐目标推送；失败写回 `cron_jobs.last_delivery_error`。
4. **会话连贯（默认）**：每个 DM 目标推送成功后，把可见正文 mirror 进该 peer 的活跃 IM 桌面 transcript（前缀 `【定时投递】`，assistant 消息 + `InjectedAssistantMessage`）。按 `session_user_id` 查找会话（兼容飞书 chat_id ≠ open_id）；群 / 无历史私聊会话则跳过。`im_send` 暂不 mirror。

> `[SILENT]` / 静默叙述：回复为 `[SILENT]`（大小写不敏感）或匹配宽口径静默正则时跳过当次推送，用于「本次无新内容可报」。

### 动态路径（im_send 工具）

`im_send` 工具让 agent 在 run 中主动推送任意消息到指定 IM 目标。参数：`to`（deliver 字符串，同上表）、`text`（消息正文，支持 markdown）。返回 `{ ok, delivered, total, errors }`。

- 已加入 `general` / `coder` / `computer` / `research` agent 的 `allowTools`。
- 由 `install_channel_outbound_bridge` 注册到 `ToolRegistry`。
- 仅主会话可见（子 Agent 不继承）；调用条件是用户点名飞书/钉钉/企微/微信或具体会话。
- cron job 已配 `deliver` 时，平台自动推送最终回复（cron 用户消息前缀已说明）。

### HTTP / Webhook 注入（Phase 2）

- `POST /api/runs`：JSON body 可带字符串字段 `deliver`（与结构化 `DeliverTarget` 枚举并存时，字符串优先写入 `trigger_meta.extra`）。
- `POST /api/webhooks/:src`：同样支持可选 `deliver`。
- Cron：`cron_jobs.last_delivery_error` 记录最近一次 IM 投递失败，成功时清空；设置页列表展示。
- `GET /api/cron-jobs/delivery-targets`（Tauri：`list_cron_delivery_targets`）：返回已配置 home channel，供 Automation 下拉；`PATCH /api/cron-jobs/:id` 可更新 `deliver`（空字符串清空）。

### home channel 配置（Phase 3a：私聊自动绑定）

`ChannelAccountConfig` 使用 `homeRecipientId` / `homeIsGroup` / `homeDisplayName`。

- **自动**：用户在某通道**私聊** Pointer 时，将该账号绑定更新为当前对方（最后一次私聊获胜）；群消息不改绑定。
- **定时任务**：`deliver` 主路径只写通道名（`feishu` / `dingtalk` / … / `all`）；设置页为「推送到 IM」+ 通道多选。
- 未绑定却指定该通道时，创建/更新任务会失败并提示先私聊。

仍可手编 `channels_config.json`；显式 `feishu:ou_xxx` 仍兼容解析，但不作为小白主路径。

### 通道主动推送兼容性

4 个通道都支持主动推送，无需额外平台改造：

| 通道 | 主动推送 | 备注 |
|------|---------|------|
| 飞书 | 可用 | `receive_target` 按 `oc_` 前缀自动区分 chat_id / open_id |
| 钉钉 | 可用 | 群推送需 `openConversationId`，解析层镜像到 `reply_context.chat_id` |
| 企微 | 可用 | WSS 连接时走 `send_markdown`；否则走 Agent HTTP `message/send` |
| 微信 iLink | 条件可用 | 需用户曾向 bot 发过消息（`context_token` 缓存命中）；否则 hook 返回清晰错误日志 |

### 装配（宿主）

server / Tauri 启动时：先建 `ChannelGateway` → 调 `install_channel_outbound_bridge(gateway, tools)`（注册 `im_send` 工具）→ 调 `AppState::build_dispatcher_with_extra_finished_hooks(vec![Arc::new(ImDeliverHook::new(gateway))])`（注册投递 hook）。Tauri 端已把 gateway 构建重排到 dispatcher 之前。

### Phase 边界

- **Phase 1（已实现）**：Cron 静态投递 + `im_send` 动态投递 + 4 通道 DM/群解析 + home channel（手编配置）+ `[SILENT]` 跳过 + 4000 字截断。
- **Phase 2（已实现）**：HTTP Runs API / Webhook ingress 注入 `deliver`；`last_delivery_error` 跟踪；`GET /api/cron-jobs/delivery-targets` + 前端下拉 / 编辑投递；宽口径静默过滤正则。
- **Phase 3a（已实现）**：每通道一条投递绑定；私聊自动绑「最后一人」；定时任务 / `cron_job` 以通道名指定推送；设置页「推送到 IM」+ 通道多选；未绑定则创建失败并提示先私聊。
- **Phase 3b（计划）**：通道设置页绑定状态展示；显示名缓存增强（飞书等 API）。
- **Phase 3c（计划）**：显式绑群、进阶自定义 deliver、会话 UI 设绑定；`DeliverTarget::Webhook`；`"origin"` 投递。