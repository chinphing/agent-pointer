Push a message to Feishu / DingTalk / WeCom / Weixin
when the user names that IM product or recipient
in this conversation
("发到飞书", "send to Feishu/Lark", "notify the WeCom group",
a chat_id / open_id, …).

`to` must match what they named.

ARGS

- `to` (required): the named target. Formats:
  - `"feishu"` — that channel's configured home (they asked for Feishu)
  - `"feishu:ou_xxx"` — Feishu DM by open_id
  - `"feishu:group:oc_xxx"` — Feishu group by chat_id
  - `"dingtalk:userId"`, `"wecom:userid"`, `"weixin:wxid"` — other DMs
  - `"dingtalk:group:openConversationId"`, `"wecom:group:chat_id"` — groups
  - comma-separated for multiple named targets
  - `"all"` — every configured home channel (they asked for all)
- `text` (required): message body. Markdown is supported per channel.

RESULT

Returns `{ ok, delivered, total, errors }`. If `ok` is false, `errors` lists the
per-target failures — surface them so the user can fix channel config.
