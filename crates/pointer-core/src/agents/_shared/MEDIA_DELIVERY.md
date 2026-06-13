## Delivering local media in chat

Write the **final** reply in **assistant message content** — there is no separate delivery tool.

### Format

- Optional short caption, then **one line per file at the end**: `MEDIA:<absolute-path>` or `MEDIA:pointer-media://…`
- **One file per line** (multiple files = multiple `MEDIA:` lines).
- **Never** paste bare paths in prose (e.g. `C:\…\out.mp4`, `/Users/…/out.mp4`). Without the `MEDIA:` prefix, users may see path text instead of a player.
- The UI renders players/thumbnails **below** the caption, not inline where a path would appear.
- Use real paths from tool output, terminal stdout, or context **Local path** — do not invent paths.

### App chat

Use `MEDIA:` + absolute path or `pointer-media://…` for inline preview in the Pointer UI.

### IM sessions (Feishu / DingTalk / WeCom / WeChat only)

Same `MEDIA:` lines at the end of assistant content. The host sends the text to the channel and delivers files as IM attachments (`MEDIA:` lines are not shown as raw path text).

### Final-reply tools (`image_generate`, `video_generate`)

On success the host **ends the turn** and delivers output automatically (inline player/gallery). **Do not** send a follow-up message repeating paths.

### Terminal-produced files

When **terminal** (or Remotion / ffmpeg / export scripts) writes a file the user should see, add short prose if needed, then a final line `MEDIA:<absolute-path-from-terminal-output>`.
