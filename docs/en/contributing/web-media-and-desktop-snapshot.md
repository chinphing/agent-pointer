# Web attachments and desktop snapshot

English | [简体中文](../../zh-CN/contributing/web-media-and-desktop-snapshot.md)

## Attachments (Web and desktop)

The Composer supports three ways to add files: pick with the paperclip, paste an image, or drop a file (the drop area is the input box panel).

| Side | Drop implementation |
| --- | --- |
| **Web** | Standard HTML5 `drop` (a `capture` listener covering the input box panel) |
| **Desktop (Tauri)** | `webview.onDragDropEvent` to obtain the local path. Tauri intercepts OS file drag-and-drop, and the event is **window-wide**; there is no coordinate hit-testing (frameless/overlay window coordinates are unreliable, see [tauri#10744](https://github.com/tauri-apps/tauri/issues/10744)), so dropping a file anywhere in the window adds it to the Composer |

The desktop side reads via local paths (large files such as videos behave the same as the file picker). Behavior is identical on macOS / Windows / Linux; the Web side reads contents through the browser `File` API.

### Pitfalls to avoid (Composer drag-in attachments)

Before changing `Composer.vue` or `tauri.conf.json`, read the **Composer file drag-and-drop** comment block inside `Composer.vue`. These are conventions that have been broken repeatedly—do not break them again:

| Don't | Why |
| --- | --- |
| Set `dragDropEnabled: false` on the main window | Mutually exclusive with Tauri's native OS drag-and-drop; on macOS the HTML5 `@drop` often does not fire for Finder files |
| Use `onDragDropEvent`'s `position` + `getBoundingClientRect` for drop hit-testing | Coordinates are relative to the window frame and do not match the viewport (an overlay title bar causes drift on the order of 28px) |
| Rely only on the template `@drop` to receive files on desktop | The WebView does not dispatch an HTML5 drop for OS file drags; you must use `onDragDropEvent` |
| Register `onDragDropEvent` separately in each Composer instance | The event is window-wide; multiple instances or remount races leak listeners, and the same file enters the shared `composerAttachments` more than once. You must go through the `lib/composerTauriDragDrop.ts` singleton + short-lived path dedup |
| Remove the `if (isTauriRuntime()) return` from the HTML5 handler | It marks the dual Web/Tauri path; otherwise it is easy to assume desktop goes through the DOM drop |
| Call `webview.scaleFactor()` | On Tauri 2 it lives on `Window`, not on `Webview` |
| Only hot-reload the frontend after changing `tauri.conf` | `dragDropEnabled` and the like take effect when the window is created; a full restart of `tauri dev` / reinstall of the package is required |

Implementation locations: `src/lib/composerTauriDragDrop.ts` (desktop singleton listener) + `Composer.vue` (subscription + Web `@drop`); for the main window's `drag_drop_enabled` see `src-tauri/tauri.conf.json` (default `true`, do not casually change it to false).

| Capability | Desktop (Tauri) | Web (pointer-server) |
| --- | --- | --- |
| Image preview | `previewChatMedia` / local path | Same as left (JSON base64 API) |
| Workspace image / PDF | `convertFileSrc` | `GET /api/workspace/file-media` → object URL (see [workspace-panel-refresh.md](../../zh-CN/ui/workspace-panel-refresh.md)) |
| Video / audio preview | Local `convertFileSrc` inline playback | **No preview**, only the file name + "Download" |
| Documents / other files | Open with the OS default app | `GET /api/chat/media-download` or `media-ref-download` (login required) |
| IM large-file external link | — | `GET /api/media/public-download?token=…` (HMAC time-limited, **no login required**) |

When you click to download an attachment on the Web side, it uses native browser navigation (`<a href>` + session cookie), and `Content-Disposition: attachment` immediately pops the download bar; the response body is streamed from disk by the server. Do **not** `fetch` the whole file into a Blob and then trigger the download (large files give no feedback for a long time, and it would hit the generic 12s request timeout). The optional query parameter `fileName` preserves the original display name (including Chinese, `filename*`).

Web-side **upload** goes through `POST /api/chat/save-attachment` (**multipart/form-data**, fields `conversationId` / `attachmentId` / `fileName` / `file`), using XHR like the video OSS upload so progress can be shown. The generic JSON API still has a 12s timeout; attachment uploads have a **120s** timeout.

Progress conventions:

- XHR `upload.onprogress` reaching **100%** only means **the browser has sent** all bytes; writing to disk / compression / OSS PutObject / URL signing on the server are all still pending before the response returns. The progress bar and label **stay at 100%**.
- While `uploadProgress >= 100` and not yet `done`, the chip shows "Processing 100%" (do not write "Uploading 100%", which looks like it finished but is stuck).
- After `uploadState === 'done'`, show "Uploaded".
- Desktop video OSS (Rust): the PutObject phase is ≤99%, and 100% is reported only after presign succeeds.

Interaction conventions:

1. After picking a file, immediately use `URL.createObjectURL` to show a chip thumbnail (no need to wait for the disk read/upload).
2. Upload starts as soon as the file is added (large images may be client-compressed first), and the chip shows progress; on send only `storageRelPath` is included, and the file/base64 is not sent again.
3. Multiple files are added and uploaded in parallel; sending is disabled while any is incomplete or failed.
4. On a network or transient server error during upload / send, retry automatically up to **3** times (about 0.8s, 1.6s apart); do not retry on login expiry, insufficient balance, missing file, **user cancel**, etc. The chip briefly shows "Retrying n/3…".
5. A chip can manually **cancel** an in-flight upload (abort the XHR on Web; on desktop the invoke cannot interrupt the underlying transfer, but the result is ignored and it is marked "Upload cancelled"); after a failure or cancel you can click **retry**; removing (×) also aborts.

On desktop the file is still written via Tauri invoke (local base64/path), and it is likewise uploaded on add with status shown.

When an IM outbound file exceeds the direct-upload limit, the server signs a `public-download` link and writes it into the channel text as Markdown (see [channel-integration.md](../developer/channel-integration.md)).

## Desktop snapshot (Web)

- API: `POST /api/computer/manual-snapshot` → the response body is a **JPEG binary stream** (`Content-Type: image/jpeg`, `Cache-Control: no-store`), no longer base64-wrapped JSON
- Format: JPEG (default quality **68**); after capture the server compresses it for preview use (long edge ≤1280px, single image ≤100KB), to avoid the 5s polling consuming too much bandwidth
- UI: the "View desktop" button in the sidebar (`DesktopSnapshotButton.vue`). It is never shown in the desktop client. On Web / standalone the server writes `pointer-desktop-snapshot` based on display detection (`1`/`0`); it is not shown when not injected
- While the preview is open, the snapshot auto-refreshes every **5 seconds**; refreshing stops when the preview is closed
- What is shown is the desktop of the **host running the pointer-server process** (a cloud ECS = a cloud desktop)
- The preview overlays a **composited mouse pointer** and an **input focus I-beam** (consistent with the Computer Agent visual overlay; on Linux the focus coordinates may be unavailable)

## ALB / readiness (platform)

See [cloud-host-integration.md](../developer/cloud-host-integration.md); for platform-side ALB / readiness details see the private repository `pointer-official/apps/api/README.md` (no public link).
