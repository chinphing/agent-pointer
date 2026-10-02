# IM channel integration guide

English | [简体中文](../../zh-CN/developer/channel-integration.md)

> For the end-user summary of the settings UI see **[`../user/im-channels.md`](../user/im-channels.md)**. The rest of this page covers full per-platform integration and Webhook deployment.

Pointer connects to Feishu, DingTalk, WeCom and WeChat through the `pointer-channels` crate, **long connections first, pure Rust**.

## Architecture

- Inbound (default): Feishu WSS, DingTalk Stream, WeCom Bot WSS, WeChat iLink long polling
- Inbound (fallback): Feishu / DingTalk / WeCom HTTP → `POST /webhooks/{channel}/{account_id}`
- Outbound: each platform's Open API / sessionWebhook active push (text + image/file)
- Orchestration: inbound message → `run_chat` → parse the `MEDIA:` markers in the reply → push text and media back separately

## Deployment

On the desktop (Tauri), **Save** only writes the channel config and does not restart the monitor; only clicking **Connect** saves and restarts the corresponding long connection. After a channel is enabled you must click Connect (or the automatic connection at the end of the QR-scan flow) before the WSS/Stream is established; **a public address is usually not required**.

Only when a channel uses **Webhook mode**:

1. Deploy `pointer-server` publicly, or expose the port with ngrok in a development environment
2. Fill in `publicBaseUrl` under Settings → **IM channels**
3. Put the generated Webhook URL into the platform's admin console

## Feishu (WSS long connection · recommended)

### One-click creation by QR scan (recommended)

1. Settings → IM channels → Feishu → click **Scan QR to create**
2. Scan the QR code with the **Feishu app** and complete app authorization as prompted
3. On success `App ID` / `App Secret` are filled in automatically; keep the connection mode at **WSS long connection**, then enable and save

### Manual configuration

1. Create a custom enterprise app and enable message send/receive permissions
2. Event subscription → choose **Receive events over a long connection**
3. Subscribe to `im.message.receive_v1`
4. In Pointer fill in `appId` and `appSecret`, choose **WSS long connection** as the connection mode, then enable and save
5. Start Pointer first (to establish the long connection), then save the event subscription config in the Feishu console
6. Logs: `feishu ws connecting`, `channel inbound channel=feishu`

### Feishu Webhook fallback

Callback URL: `{publicBaseUrl}/webhooks/feishu/default`; `encryptKey` must be filled in.

## DingTalk (Stream long connection · recommended)

### One-click creation by QR scan (recommended)

1. Settings → IM channels → DingTalk → click **Scan QR to create**
2. Scan the QR code with the **DingTalk app** and click "Create a new bot in one click"
3. On success `Client ID` / `Client Secret` are filled in automatically; keep the connection mode at **Stream long connection**, then enable and save

### Manual configuration

1. Create an enterprise internal app bot and choose **Stream mode** for message receiving
2. In Pointer fill in `clientId` (AppKey) and `clientSecret`, choose **Stream long connection** as the connection mode, then enable and save
3. Group chats require @-mentioning the bot; direct chats just send a message
4. Logs: `dingtalk stream connected`, `channel inbound channel=dingtalk`

### DingTalk HTTP fallback

Callback URL: `{publicBaseUrl}/webhooks/dingtalk/default`

## WeCom

### WSS long connection (recommended · smart bot)

#### One-click creation by QR scan (recommended)

1. Settings → IM channels → WeCom → click **Scan QR to create**
2. Scan the QR code on the page with the **WeCom app** and click "Create a smart bot in one click"
3. On success `Bot ID` / `Secret` are filled in automatically; keep the connection mode at **WSS long connection**, then enable and save

As with Feishu/DingTalk: the backend calls WeCom `/ai/qc/gen` to obtain the authorization link and generate the QR code, then polls `/ai/qc/query_result` until `botId` / `secret` are returned.

#### Manual configuration

1. Create a **smart bot** in the WeCom admin console and get `botId` + `secret`
2. Settings → IM channels → WeCom → choose **WSS long connection**
3. Fill in `botId` and `secret` (you may change `websocketUrl`; default `wss://openws.work.weixin.qq.com`)
4. After saving, restart the app / `pointer-server` (the monitor connects at startup)
5. **No public Webhook required**; the Tauri desktop client can also connect to WSS directly

### HTTP callback (custom app Agent)

1. Custom app → Receive messages → set the callback URL
2. URL: `{publicBaseUrl}/webhooks/wecom/default`
3. Configure `corpId`, `agentId`, `secret`, `token`, `encodingAesKey`

### Group @ and dynamic agents (WeCom)

**Group @**: in Bot WSS mode the WeCom platform pushes a message only when the user @-mentions the bot; Pointer strips the `@bot name` prefix from the message. In Agent HTTP callback mode this is decided by whether the text starts with `@`; when `requireMention: true` is set, group messages without an @-mention are dropped.

**Dynamic agents** (`dynamicAgents`, enabled by default):

| Field | Default | Description |
| --- | --- | --- |
| `enabled` | `true` | Enable dynamic conversation routing |
| `dmCreateAgent` | `true` | Isolate direct-message conversations per user |
| `groupEnabled` | `true` | Share one conversation per group chat (not per sender) |
| `adminUsers` | `[]` | userids in the list use the main conversation `_main` and are not isolated |

When a group chat shares one conversation, inbound messages automatically get a `[userid]:` prefix so the model can tell speakers apart. Example config:

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

## WeChat

1. Click "Scan QR to log in" in settings
2. Scan with the **WeChat app** (the QR code content is the liteapp link returned by `qrcode_img_content`, not the polling token)
3. After confirmation on the phone, the credentials are stored encrypted locally under `channel_credentials/`
4. Tick "Enable" and save; the desktop then automatically starts the iLink `getupdates` long-polling monitor
5. The first direct message requires pairing by default (`dmPolicy: pairing`), or approve the pairing code in settings

## Switching agents inside an IM conversation

In a WeCom / Feishu / DingTalk / WeChat IM conversation you can send the **agent display name** or **id** directly to switch modes (no fixed command required):

| Example input | Effect |
| --- | --- |
| `电脑操控` (Computer control) | Switch to Computer control |
| `通用助手` (General assistant) | Switch to General assistant |
| `氛围编程` (Vibe coding) | Switch to Vibe coding |
| `电脑操控 帮我打开浏览器` (Computer control: open the browser for me) | Switch first, then handle the following request |

Inside an IM conversation only the three agents above are exposed; other built-in agents (such as deep research) cannot be switched to with an IM command.

A successful switch replies with `已切换到…。` ("Switched to ….").  
Conversation reset is still supported: `/new`, `/reset`, `新对话` ("new chat"), `重新开始` ("start over").

Each IM conversation remembers its current agent independently (a conversation-level `leadAgentId`); it does not affect other desktop conversations. When you switch agents in IM, the corresponding conversation in the desktop sidebar and the Composer update in sync.

Vibe coding working directory: choose a project folder for that conversation on the desktop (conversation-level `workspaceRoot`); if unset, the default conversation sandbox is used (keyed by `session_user_id` or the anonymous conversation directory, see [workspace-root.md](workspace-root.md)). After the user clears it with ✕ in the Composer, the default sandbox is used instead; the sandbox directory is created only **when the first message is sent**, so clearing does not immediately create a directory on disk.

## The desktop mirrors IM conversations

When an IM inbound message triggers `run_chat`, stream events (tool calls, reasoning, sub-agent traces, etc.) are broadcast to the main Pointer UI, and the corresponding IM conversation appears in the sidebar, where you can watch the intermediate process and the final reply in real time (just like a desktop chat).

## Outbound media (images / files)

All four IM channels support the agent sending images or files to the user. The model appends media path lines at the end of the reply (aligned with the OpenClaw `MEDIA:` convention); dispatch parses and uploads them automatically; these lines are **not** shown to the user.

**Charts:** a Markdown ` ```chartjs ` / ` ```chart ` fence (Chart.js JSON) in the reply is normalized by the host before outbound (the same soft palette + white background as the app, illegal callback strings removed), then rasterized to PNG with a **system Chinese font** (`generated-media/im-charts/`) and rewritten into a `MEDIA:` attachment for sending (the font path can be overridden with `POINTER_IM_CHART_FONT`). Multiple Y axes (`y1`/`y2`/…): fulgur does not support secondary axes, so the host maps each secondary-axis series onto the primary axis with its own scale to align the curve shapes (the legend is annotated `y1 尺度…` and so on; secondary-axis ticks are not drawn). Desktop/Web still show an interactive chart. Implementation: `chart_outbound.rs` / `im_chart_font.rs`; see [`../ui/markdown-charts.md`](../../zh-CN/ui/markdown-charts.md).

Supported forms:

```
This is the analysis result.
MEDIA:pointer-media://{convId}/{attachmentId}.png
MEDIA:pointer-media://generated-media/{user}/{conv}/{uuid}.png
MEDIA:/absolute/path/to/report.pdf
pointer-media://{user}/{convId}/{file}.md
```

A bare `pointer-media://…` line on its own (no `MEDIA:` prefix) is likewise sent as an outbound attachment when **the file resolves**; if resolution fails it stays in the visible body, which makes troubleshooting easier.

Absolute paths may contain spaces (e.g. macOS `…/Library/Application Support/…`). The host parses same-line forms (such as `- 文档：MEDIA:/…/Application Support/…/x.md`) as **the whole path**; the old logic used `[^\s]+` and truncated at the first space. When the whole segment does not exist, it falls back to the first token only if what follows the space **does not look like a path continuation** (e.g. `MEDIA:/tmp/a.png world`); if the continuation contains `/` or `\` (`Application` + `Support/…`), it must not be treated as an attachment even when the 0-byte prefix file `…/Library/Application` exists. If the file does not exist it is **not attached**, and by convention the `MEDIA:` line **must not be stripped from the body** (`5df5847` / `split_reply_media`: extraction happens only when the file really exists). **A 0-byte empty file also counts as a valid file** (`Path::is_file()`); it is not rejected because its size is 0. The path may optionally be wrapped in `` ` `` / `"` / `'`; when the closing quote is missing, parsing falls back to an unquoted path. Implementation: `media/outbound_reply.rs`.

Path resolution order:

1. `pointer-media://` or a `conversation-media/` relative path → a saved attachment under the app data directory
2. `generated-media/`, `session-sandboxes/` prefixes → the corresponding subdirectory under `{app_data}/` (AI-generated images/videos, conversation sandbox output)
3. Absolute path → read directly
4. Relative path → try the data directory, then the workspace

For app-managed paths (storage rel, `generated-media/`, `session-sandboxes/`), outbound resolution (`resolve_outbound_media`) treats a successful local resolution as deliverable; only workspace-relative paths additionally have to fall inside `app_data`, so that server mode does not read files outside the local cwd.

Send behaviour:

| Channel | Text | Image | File |
| --- | --- | --- | --- |
| Feishu | post markdown | `im/v1/images` + image message | `im/v1/files` + file message |
| DingTalk | sessionWebhook markdown | media/upload + image | media/upload + file |
| WeCom WSS | streaming / markdown | WS chunked upload + image/file message | same as above |
| WeCom Agent HTTP | text | media/upload + message/send | same as above |
| WeChat iLink | text item | CDN encrypted upload + image_item | CDN encrypted upload + file_item / video_item |

WeChat image outbound: `image_item.mid_size` / `hd_size` must be filled with the **AES ciphertext length** (not the plaintext PNG size), and `media.aes_key` uses `base64(hex key)` (same as OpenClaw/Hermes); if filled in wrongly you get "the message is sent but the image is broken on the client". Chart PNGs go through the same upload path.

WeChat iLink outbound caches the `context_token` of every inbound message (per account + user). For long-running agents: when the cache is older than **45 seconds**, `getconfig` is called before sending to try to refresh (first with the old token, then without a token); if `sendmessage` returns `ret=-2 errmsg=unknown`, it refreshes again and retries once (accepting the case where `getconfig` returns the same token). If the user has not sent a message for a long time and refresh still fails, the user must send another message to activate the conversation.

Single-file limit: non-video **30 MB**; **video** matches Composer OSS (**5 GB** hard limit, **>500 MB** automatically compressed before upload). If the path cannot be resolved or the upload fails, an error is logged and the text reply is still sent.

**IM large-file outbound (all channels):** above the IM direct-upload limit (**20 MB**, aligned with WeCom `40006`), the platform attachment upload is no longer attempted; instead an **HMAC time-limited download link** is issued (`GET /api/media/public-download?token=…`, no login required) and sent to the user as Markdown link text (like `📎 [file.zip](https://…)`). A failed direct upload also falls back to the same link. A publicly reachable `POINTER_SERVER_PUBLIC_URL` (or channels `publicBaseUrl`) must be configured. Optional: `POINTER_MEDIA_DOWNLOAD_SECRET`, `POINTER_MEDIA_DOWNLOAD_TTL_SECS` (default 7 days, max 7 days). Single-link file limit **512 MB**.

### Inbound self-message filtering (avoiding an extra run)

Some IM platforms push **messages sent by the bot itself** back to the app; without filtering they are treated as new user messages and trigger an extra `run_chat` turn (a typical Feishu case: send text, then send a file → the echoed text triggers another turn).

| Channel | Does the platform echo self-messages? | How this project handles it |
| --- | --- | --- |
| Feishu | Yes (`sender_type` is `app` / `bot`) | `feishu/parse.rs` drops self-messages |
| WeChat iLink | Usually not (`message_type=2` is BOT) | Only `message_type=1` is accepted; senders matching `*@im.bot` are dropped as well |
| WeCom WSS | No (docs: `aibot_send_msg` does not trigger a callback) | Only `aibot_msg_callback` user messages are handled |
| DingTalk Stream | Usually not (the topic is user → bot) | Self-messages are dropped when `senderId == chatbotUserId` |

### Outbound replies (aligned with OpenClaw)

In an IM conversation the model writes its **final body into the assistant message**, and the host sends it automatically to the corresponding IM channel after `run_chat` ends; no separate delivery tool is needed (this project removed the earlier `channel_message` tool to avoid duplicating the final reply).

**Intermediate turn push** (Settings → IM channels → **IM outbound push**):

| Option | Default | Description |
|------|------|------|
| `sendIntermediateText` | On | Every model output (`MessageEnd` with a body) is pushed to the IM client immediately |
| `sendToolCalls` | On | Only pushes the tool **call start** (`🔧 label`); completion/failure status lines are **not** pushed |

`ask_user` clarification options are pushed proactively by the host (a Hermes-style numbered list) and do not go through tool status lines.

If the final reply is identical to intermediate text already pushed, it is not sent again; `MEDIA:` attachments are sent by resolvable local path (`pointer-media://…`, absolute paths and relative paths all work; `..` path traversal is forbidden).

### Manual send API (aligned with `openclaw message send --media`)

```bash
curl -X POST http://127.0.0.1:8787/api/channels/feishu/default/send \
  -H 'Content-Type: application/json' \
  -d '{"recipientId":"ou_xxx","text":"hi","mediaUrls":["/path/file.png"]}'
```

DingTalk requires `sessionWebhook`; WeChat requires `contextToken`; a WeCom Bot passive reply may carry `wecomReqId`.

## Pairing

The DM policy defaults to `pairing`. Unknown users receive a pairing code, which an administrator approves by entering it in settings.

The desktop/web client does **not** poll the pending API continuously while idle: it polls for a short time (about 4s) only when a pairing code is issued (stream event `channel_pairing_pending`) or when unexpired pending entries are found at startup; polling stops when pending is empty or the backend TTL (5 minutes) is exceeded.

## Conversation reset

The model context of an IM channel and the desktop conversation in the app can be reset by a chat command or an idle timeout. After a reset a **new blank conversation** is created in the sidebar (its title carries `新对话` ("new chat")); the old conversation record stays in the list for later review; new IM messages go to the new conversation.

### Stopping the current task

Aligned with the OpenClaw `/stop` **fast abort path**: sending the following commands while a task is running cancels the current `run_chat` **immediately** (without queueing) and replies `已停止当前任务。` ("Current task stopped."):

- `/stop`, `/cancel`, `/abort`
- `停止` ("stop"), `停下来` ("stop"), `暂停` ("pause")

Only one task runs at a time in the same IM conversation (a new message waits for the current task to finish; after stopping you can send a new message immediately).

### `ask_user` (aligned with Hermes `clarify`)

In IM, `ask_user` **blocks the same turn** (about 600 seconds max by default):

1. Before `ask_user` blocks, the host proactively pushes Hermes-style clarification text (question + numbered options + a reply hint)
2. After calling `ask_user` it waits for the user's next message
3. `ChannelGateway::process_inbound` intercepts before starting a new turn: if it can map, it maps the number/label; otherwise, following Hermes, the raw text is treated as the answer → filled into the tool result
4. The model keeps running in the same turn; only an empty message prompts again and keeps waiting

`/stop` cancels the block. On timeout the tool returns `timed_out: true` and the model uses its own default or asks again.  
App / Web interaction offers an "Other" free-text input outside the given options; the backend, like IM, accepts raw text outside the options.

### Manual reset

Sending any of the following in IM clears the current conversation history (the old record is archived, not deleted outright):

- `/new`, `/reset`
- `新对话` ("new chat"), `重新开始` ("start over")

When only the reset command is sent, the bot replies `已开始新对话。` ("Started a new conversation.") and the command is **not** handed to the model; the desktop switches to a new blank sidebar conversation.  
If extra text follows (such as `/new 帮我查天气`), it resets and forks a new sidebar conversation first, then handles the remaining content.

A manual reset does **not** backfill history messages from the channel API.

### Idle auto-reset

All IM channels share one global setting (`channels_config.json` → `meta`):

```json
"meta": {
  "sessionReset": { "idleMinutes": 60 }
}
```

By default, after **60 minutes** (1 hour) without a new message, the next inbound message forks a new desktop conversation (`@sN`); the old fork remains in `conversations.db`.  
`idleMinutes` set to `0` disables it. Settings page → IM channels → **Session reset** lets you change it (it applies to all channels).

## Configuration storage

- Channel config: `{data_dir}/PointerApp/channels_config.json`
- Credentials: `{data_dir}/PointerApp/channel_credentials/` (encrypted)
- Conversation history and IM metadata: `{data_dir}/PointerApp/conversations.db` (shared with desktop conversations)
- IM thread state (`sessionEpoch`, `activeConversationId`, the `updatedAt` used for idle) is stored in the `conversations` table fields of the base conversation row
- The legacy `channel_histories/` is renamed to `channel_histories.deprecated/` on first startup (only the `activeConversationId` metadata inside it is imported)

## Run → IM outbound bus

Push the final reply of any run (or a message constructed proactively during the run) to an IM channel. It covers both the **static** path (the delivery target is configured at trigger time) and the **dynamic** path (the agent decides during the run). The framework lives in `pointer-channels`; the trigger-side injection lives in `pointer-core`.

### The deliver string format

A delivery target is described by one string, stored in `trigger_meta.extra.deliver` (persisted to `runs.trigger_meta_json`). Format:

| Form | Meaning |
|------|------|
| `feishu` | The Feishu account's home channel (requires `homeRecipientId` in `channels_config.json`) |
| `feishu:ou_xxx` | Feishu DM (by open_id) |
| `feishu:group:oc_xxx` | Feishu group (by chat_id; the `oc_` prefix is recognized automatically) |
| `dingtalk:userId` | DingTalk DM (via `oToMessages/batchSend`) |
| `dingtalk:group:openConversationId` | DingTalk group (via `groupMessages/send`; openConversationId is mirrored to `reply_context.chat_id`) |
| `wecom:userid` | WeCom DM (Agent HTTP `message/send`) |
| `wecom:group:chat_id` | WeCom group (WSS `send_markdown(chat_id)`) |
| `weixin:wxid` | WeChat iLink DM (the user must have sent a message to the bot before; only a `context_token` cache hit can push) |
| `a,b,c` | Comma-separated multiple targets |
| `all` | All channels with a configured home channel |

Resolution happens in `pointer-channels/src/im_delivery.rs::resolve_delivery_targets`; per-channel differences are absorbed in the resolution layer, and hooks and the `im_send` tool only call `ChannelGateway::send_outbound_explicit`.

### Static path (Cron)

1. The `cron_jobs.deliver` column stores the deliver string (filled in when creating a job with the `cron_job` tool / the Automation panel in settings).
2. `scheduler.rs dispatch_job` injects `job.deliver` into `TriggerRequest.trigger_meta.extra.deliver` and sets the `DeliverTarget::Im` marker bit; the user message is prefixed by `build_cron_user_prompt` with a Hermes-style `[IMPORTANT…]` (do not use `im_send` / `[SILENT]`), and **no** cron system block is injected any more.
3. When the run ends → `ImDeliverHook` (`OnRunFinishedHook`) reads `trigger_meta.extra.deliver`, loads the last assistant reply, splits media / skips silent narration / truncates to 4000 characters, and pushes to each target; a failure is written back to `cron_jobs.last_delivery_error`.
4. **Conversation continuity (default)**: after each DM target is pushed successfully, the visible body is mirrored into that peer's active IM desktop transcript (prefixed `【定时投递】` (Scheduled delivery), as an assistant message + `InjectedAssistantMessage`). The conversation is looked up by `session_user_id` (tolerating Feishu chat_id ≠ open_id); groups / direct-message conversations without history are skipped. `im_send` does not mirror for now.

> `[SILENT]` / silent narration: when the reply is `[SILENT]` (case-insensitive) or matches the broad silent regex, that push is skipped, for "nothing new to report this time".

### Dynamic path (the `im_send` tool)

The `im_send` tool lets the agent push any message to a specified IM target during a run. Parameters: `to` (a deliver string, as in the table above), `text` (the message body, Markdown supported). Returns `{ ok, delivered, total, errors }`.

- Added to the `allowTools` of the `general` / `coder` / `computer` / `research` agents.
- Registered into the `ToolRegistry` by `install_channel_outbound_bridge`.
- Visible only in the main conversation (sub-agents do not inherit it); call it when the user names Feishu/DingTalk/WeCom/WeChat or a specific conversation.
- When a cron job already has `deliver` configured, the platform pushes the final reply automatically (the cron user-message prefix explains this).

### HTTP / Webhook injection (Phase 2)

- `POST /api/runs`: the JSON body may carry a string field `deliver` (when it coexists with the structured `DeliverTarget` enum, the string is written to `trigger_meta.extra` first).
- `POST /api/webhooks/:src`: likewise supports an optional `deliver`.
- Cron: `cron_jobs.last_delivery_error` records the most recent IM delivery failure and is cleared on success; the settings list shows it.
- `GET /api/cron-jobs/delivery-targets` (Tauri: `list_cron_delivery_targets`): returns the configured home channels for the Automation dropdown; `PATCH /api/cron-jobs/:id` can update `deliver` (an empty string clears it).

### home channel configuration (Phase 3a: automatic direct-message binding)

`ChannelAccountConfig` uses `homeRecipientId` / `homeIsGroup` / `homeDisplayName`.

- **Automatic**: when a user **direct-messages** Pointer on a channel, that account's binding is updated to the current peer (last direct message wins); group messages do not change the binding.
- **Scheduled tasks**: the main `deliver` path writes only the channel name (`feishu` / `dingtalk` / … / `all`); the settings page offers "Push to IM" + a channel multi-select.
- If the channel is specified but nothing is bound, creating/updating the job fails and prompts you to send a direct message first.

You can still hand-edit `channels_config.json`; an explicit `feishu:ou_xxx` is still parsed for compatibility, but it is not the main path for beginners.

### Channel active-push compatibility

All 4 channels support active push with no extra platform work:

| Channel | Active push | Notes |
|------|---------|------|
| Feishu | Available | `receive_target` distinguishes chat_id / open_id automatically by the `oc_` prefix |
| DingTalk | Available | Group push requires `openConversationId`; the resolution layer mirrors it to `reply_context.chat_id` |
| WeCom | Available | Uses `send_markdown` when the WSS connection is up; otherwise Agent HTTP `message/send` |
| WeChat iLink | Conditionally available | Requires that the user has sent a message to the bot before (`context_token` cache hit); otherwise the hook returns a clear error log |

### Wiring (host)

At server / Tauri startup: first create the `ChannelGateway` → call `install_channel_outbound_bridge(gateway, tools)` (registers the `im_send` tool) → call `AppState::build_dispatcher_with_extra_finished_hooks(vec![Arc::new(ImDeliverHook::new(gateway))])` (registers the delivery hook). On the Tauri side the gateway construction was reordered to happen before the dispatcher.

### Phase boundaries

- **Phase 1 (implemented)**: Cron static delivery + `im_send` dynamic delivery + 4-channel DM/group resolution + home channel (hand-edited config) + `[SILENT]` skip + 4000-character truncation.
- **Phase 2 (implemented)**: HTTP Runs API / Webhook ingress injecting `deliver`; `last_delivery_error` tracking; `GET /api/cron-jobs/delivery-targets` + frontend dropdown / delivery editing; broad silent-filter regex.
- **Phase 3a (implemented)**: one delivery binding per channel; direct messages bind "the last person" automatically; scheduled tasks / `cron_job` specify the push by channel name; settings "Push to IM" + channel multi-select; if unbound, creation fails and prompts you to send a direct message first.
- **Phase 3b (planned)**: show the binding status on the channel settings page; richer display-name caching (Feishu and other APIs).
- **Phase 3c (planned)**: explicit group binding, advanced custom deliver, setting the binding in the conversation UI; `DeliverTarget::Webhook`; `"origin"` delivery.
