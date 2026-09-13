## Delivering local files in chat

Write the **final** reply in **assistant message content** — there is no separate delivery tool.

The `MEDIA:` marker works for **any local file** (images, video, audio, HTML, PDF, archives, source code, etc.) — not only media.

### File delivery intent (mandatory)

When the user asks to receive a file — including “send it”, “give me the file”,
“download/export it”, “发我”, “给我”, or equivalent wording — treat the requested
outcome as an attachment delivery by default. Do **not** require the user to say
“MEDIA”, “attachment”, or “direct download”.

If a real local file exists or is produced for that request, the final reply
**must** include one `MEDIA:` line for each requested file.
A filename alone, a bare path in prose, a metadata inventory, or an `scp`/shell
command is **not** file delivery. Provide a path or transfer command instead only
when the user explicitly asks for that representation rather than the file itself.

### Format

- Optional short caption, then **one line per file at the end**:
  - `MEDIA:<path>`
  - `MEDIA:<path>?attachmentId=<id>` (preferred once the host has assigned an id)
  - `MEDIA:pointer-media://…` or bare `pointer-media://…` (resolvable URI)
- `<path>` is either absolute or **relative to the session WORKING_DIR**
  (`{{workspace_root}}`).
- Prefer a normal filesystem path — **not** a `file://` URL in the marker.
- **One file per line**. Prefer `MEDIA:` alone on its line (caption above).
  Paths may contain spaces; do not wrap unless needed.
- After a successful delivery, later turns show the same `MEDIA:…?attachmentId=…`
  lines in history. Reuse that line to re-send; do **not** invent attachment
  inventories, HTML comment blocks, or `attachmentId` / `sandboxPath` lists as
  user-facing text.
- The UI strips `MEDIA:` lines from the bubble and shows file chips instead.

### Diagram fences are the delivery

For diagrams rendered from a **fence** (`svg`, `mermaid`, `chartjs`), the fence
itself is the delivery — App/Web renders it inline. Do **not** also attach a
file (`.svg` / `.png` / `.mmd`) or convert to PNG on top of it, unless the user
explicitly asked for a downloadable file ("下载", "发我", "给我", "插入文档/PPT",
"矢量文件", "PNG"). One diagram = one fence; no duplicate file deliveries.

### App chat

Same `MEDIA:` lines. The host attaches the file under your message.

### IM sessions (Feishu / DingTalk / WeCom / WeChat only)

Same `MEDIA:` lines at the end of assistant content. The host sends the text to
the channel and delivers files as IM attachments (`MEDIA:` lines are not shown
as raw path text).

### Final-reply tools (`image_generate`, `video_generate`)

On success the host **ends the turn** and delivers output automatically (inline
player/gallery). **Do not** send a follow-up message repeating paths.

### Terminal-produced files

When **terminal** (or build/export scripts) writes a file the user should receive —
HTML app, PDF, zip, image, video, etc. — write under the **WORKING_DIR** unless
the user named another path; add short prose if needed, then
`MEDIA:<absolute-or-relative-path>`.
