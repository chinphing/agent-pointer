# Attachment storage

New attachments are stored in the default user sandbox, not in
`conversation-media/` or `generated-media/`.

## Layout

- Signed-in user:
  `{app_data}/session-sandboxes/{session_user_id}/attachments/`
- Anonymous conversation:
  `{app_data}/session-sandboxes/_anonymous/{conversation_id}/attachments/`

Each filename is `{short-id}_{safe-file-name}`. `short-id` is exactly
12 lowercase hexadecimal characters.

## IDs and references

The short ID is the persisted `MediaAttachment.id` for newly saved files.
It is the preferred value for model and tool attachment operations.

`storageRelPath` remains an app-data-relative path, for example:

```text
session-sandboxes/user-42/attachments/0a1b2c3d4e5f_report.pdf
```

Use `storageRelPath` or the resolved absolute path only when a file API
requires a path. Do not derive a file path from a display filename.

## Allocation

The writer reserves a filename with an atomic create operation. The initial
ID is a random 48-bit value; a collision advances to the next hexadecimal
value until an unused filename is reserved. This works across concurrent
desktop, web, and channel writes sharing the same sandbox filesystem.

## Desktop upload

Desktop Composer prefers **path copy** when the user picks or drops a local file:

1. Chip preview uses `convertFileSrc` for images (no full-file base64).
2. Host command `save_chat_attachment_from_path` streams the file into the
   session sandbox (`media::store::save_attachment_from_path`).
3. Legacy `save_chat_attachment` (base64 IPC) remains for paste/`File` fallbacks
   and older drafts without a local path.

Do not reintroduce whole-file base64 on the desktop pick/drop path — it doubles
memory and IPC cost for large attachments.

## Web upload

`POST /api/chat/save-attachment` accepts **multipart/form-data**:

| Field | Required | Notes |
|-------|----------|--------|
| `conversationId` | yes | Session id |
| `attachmentId` | yes | Client attachment id (short id assigned on save) |
| `fileName` | yes* | Optional if multipart filename is present |
| `file` | yes | Raw file bytes |

Non-video uploads are capped by the user setting **`attachmentUploadMaxBytes`**
(Settings → system → attachment upload). Default **100 MB**, allowed range
**1–512 MB**. The HTTP route body limit is the range ceiling plus multipart
overhead; the configured value is what actually accepts or rejects a file.
Video still uses the OSS / 5 GB path and is not limited by this setting.

Prefer uploading when the user adds the file (Composer), then send chat with
`storageRelPath` only. Composer and `sendChat` auto-retry transient failures
up to 3 attempts.

## Compatibility

Historical `conversation-media/` and `generated-media/` files remain
readable. They are not moved automatically. New writes must use the sandbox
attachment layout through `media::store::save_attachment_bytes` or
`save_attachment_from_path`.

