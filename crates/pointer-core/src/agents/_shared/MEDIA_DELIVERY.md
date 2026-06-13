## Delivering local files in chat

Write the **final** reply in **assistant message content** — there is no separate delivery tool.

The `MEDIA:` marker works for **any local file** (images, video, audio, HTML, PDF, archives, source code, etc.) — not only media.

### Format

- Optional short caption, then **one line per file at the end**: `MEDIA:<absolute-path>` or `MEDIA:pointer-media://…`
- Use a normal filesystem path (e.g. `C:\Users\…\minesweeper.html`, `/Users/…/out.pdf`) — **not** a `file://` URL in the marker.
- **One file per line** (multiple files = multiple `MEDIA:` lines).
- **Never** paste bare paths or `file://` links in prose (e.g. `C:\…\out.html`, `file:///C:/…/game.html`). Without `MEDIA:`, users may see path text instead of an attachment chip or player.
- The UI renders previews **below** the caption: images/video/audio inline; other files as a **named attachment** the user can open with the default app.
- Use real paths from tool output, terminal stdout, or context **Local path** — do not invent paths.

### App chat

Use `MEDIA:` + absolute path or `pointer-media://…`. The host attaches the file under your message.

### IM sessions (Feishu / DingTalk / WeCom / WeChat only)

Same `MEDIA:` lines at the end of assistant content. The host sends the text to the channel and delivers files as IM attachments (`MEDIA:` lines are not shown as raw path text).

### Final-reply tools (`image_generate`, `video_generate`)

On success the host **ends the turn** and delivers output automatically (inline player/gallery). **Do not** send a follow-up message repeating paths.

### Terminal-produced files

When **terminal** (or build/export scripts) writes a file the user should receive — HTML app, PDF, zip, image, video, etc. — add short prose if needed, then `MEDIA:<absolute-path-from-terminal-output>`. Prefer delivery over telling the user to run a local server or open a `file://` URL themselves.
