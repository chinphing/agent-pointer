---
schema:
  type: object
  properties:
    action:
      type: string
      enum: [send]
    text:
      type: string
    message:
      type: string
    media:
      type: string
    mediaUrls:
      type: array
      items:
        type: string
    path:
      type: string
    filePath:
      type: string
  required:
    - action
  additionalProperties: true
---

### channel_message

Send a message to the current IM user (Feishu / DingTalk / WeCom / Weixin).

Only available during an active IM channel session.
Do not use in the desktop or web app chat.

#### Parameters

- **`action`** (required) — `send`
- **`text`** or **`message`** — visible text (optional if sending media only)
- **`media`**, **`path`**, **`filePath`**, or **`mediaUrls`** — local file path(s) to send as image/file attachments

Paths may be absolute, workspace-relative, `pointer-media://…`, or under configured `mediaLocalRoots`.

#### Example

```json
{
  "action": "send",
  "text": "Here is the report.",
  "mediaUrls": ["/Users/me/report.pdf"]
}
```
