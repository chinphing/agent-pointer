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

## Compatibility

Historical `conversation-media/` and `generated-media/` files remain
readable. They are not moved automatically. New writes must use the sandbox
attachment layout through `media::store::save_attachment_bytes`.
