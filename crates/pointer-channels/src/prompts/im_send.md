Send a message to an IM channel (Feishu / DingTalk / WeCom / Weixin) from inside
a run. Use this when the user explicitly asks you to push a result, alert, or
notification to an IM channel — or when a workflow needs to notify someone
outside the current chat.

WHEN TO USE

- The user says "send this to my Feishu" / "发到飞书" / "notify the group".
- A background workflow needs to push an alert or summary to an IM channel.
- You are a cron / automated run WITHOUT a `deliver` config and you decide
  mid-run that a specific channel should be notified.

WHEN NOT TO USE

- You are a cron run whose `deliver` field is already set — the platform
  auto-delivers your final reply; calling `im_send` would duplicate it.
  (The cron prompt tells you when this is the case.)
- The user just wants the answer in the current chat — reply normally instead.

ARGS

- `to` (required): delivery target. Formats:
  - `"feishu"` — the channel's configured home channel
  - `"feishu:ou_xxx"` — Feishu DM by open_id
  - `"feishu:group:oc_xxx"` — Feishu group by chat_id
  - `"dingtalk:userId"`, `"wecom:userid"`, `"weixin:wxid"` — other channels' DMs
  - `"dingtalk:group:openConversationId"`, `"wecom:group:chat_id"` — groups
  - comma-separated for multiple targets, e.g. `"feishu:ou_a,feishu:group:oc_b"`
  - `"all"` — every configured channel's home channel
- `text` (required): message body. Markdown is supported per channel.

RESULT

Returns `{ ok, delivered, total, errors }`. If `ok` is false, `errors` lists the
per-target failures — surface them to the user so they can fix the channel
config (missing token, unknown recipient, etc.).
