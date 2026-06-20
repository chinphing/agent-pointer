---
schema:
  type: object
  properties:
    ref:
      type: string
      description: >
        Media reference: pointer-media:// URI, conversation-media relative path,
        or absolute local path from the user attachment manifest.
        For mode=image, may also be a directory path containing image files.
    mode:
      type: string
      enum:
        - image
        - video
        - audio
        - pdf
      description: Understanding mode. Use **audio** for speech from an audio or **video**
        ref (host extracts the track). Use **video** for **visual** content only — scenes,
        UI, actions; it does **not** process the audio track. Must match the user task.
    goal:
      type: string
      description: >
        Required. What the user wants from this file: analysis goal, focus, output shape,
        and relevant conversation context (language, scope, constraints).
        Paraphrase from the user's latest message — do not call with only ref/mode.
    context:
      type: string
      description: >
        Optional extra background: prior turns, file nickname, section/page hints,
        or constraints not already in goal.
    pageStart:
      type: integer
      minimum: 1
      description: >
        PDF only. 1-based first page to extract. Omit unless the user explicitly asked
        for specific pages; host defaults to pages 1–10.
    pageEnd:
      type: integer
      minimum: 1
      description: >
        PDF only. 1-based last page (inclusive). Use with pageStart when the user
        named a page range. Max 10 pages per call — split larger ranges across calls.
    imageStart:
      type: integer
      minimum: 1
      description: >
        Image directory only. 1-based first image index (sorted by file name).
        Omit unless the user explicitly asked for a range; host defaults to images 1–200.
    imageEnd:
      type: integer
      minimum: 1
      description: >
        Image directory only. 1-based last image index (inclusive). Max 200 images
        per call — split larger ranges across calls.
    timeStartSec:
      type: number
      minimum: 0
      description: >
        Video only, ffmpeg fallback path. Start of the time window in seconds.
        Omit on the native OSS **video_url** path — put segment focus in **goal** instead.
        On fallback, omit unless the user explicitly asked for a segment; host defaults to
        the first segment (up to 200s at 1 fps).
    timeEndSec:
      type: number
      minimum: 0
      description: >
        Video only, ffmpeg fallback path. End of the time window in seconds.
        Use with timeStartSec when the user named a segment. Max 200 frames per call.
    framesPerSecond:
      type: number
      minimum: 0.1
      description: >
        Video only. Frames per second. On native OSS path, controls server-side sampling **fps**.
        On ffmpeg fallback, sampling inside the time window. Omit for host default of 1 fps.
  required:
    - ref
    - mode
    - goal
  additionalProperties: false
---

Understand image, video, audio, or PDF files on demand via host-managed models.

## Parameters

- **ref** + **mode** — from the manifest, or a **local directory path** (image mode only).
- **goal** (required) — user's analysis goal in their language.
- **context** (optional) — extra thread background not already in **goal**.
- **pageStart** / **pageEnd** (PDF) — only when the user explicitly asked for pages.
- **imageStart** / **imageEnd** (image directory) — only when the user explicitly asked.
- **timeStartSec** / **timeEndSec** / **framesPerSecond** (**video**, ffmpeg fallback only) —
  only when the user explicitly asked for a segment or sampling rate on the frame path.

## Modes

- **image** / **video**: vision model (frame sampling). **video** = scenes, UI, actions,
  on-screen text (host may use OSS `video_url` or ffmpeg frames).
  **Does not read the audio track** — no transcript, no spoken-word analysis.
- **audio**: ASR model. **Speech / transcript** from an audio **or video** attachment.
  Pass the same **ref** from the manifest; for video files the host **extracts the audio
  track** then runs ASR. **Does not see the picture.**
- **pdf**: sorted text extraction; scanned pages fall back to page-image OCR.

## Video attachments: vision vs speech

**video** mode analyzes **frames only**. Spoken words, music, and sound effects are
**not** available to the vision model — even when the file is a normal `.mp4` with sound.

**audio** mode analyzes **sound only**. For a **video** ref the host runs this pipeline:

1. Resolve the attachment from the manifest (**ref**) and read the **local file**.
2. **Extract the audio track** from the video file (ffmpeg).
3. Run speech-to-text (ASR).
4. Return text shaped by **goal** (verbatim transcript, summary, speaker labels, etc.).

**audio** does **not** use OSS **remoteUrl** — only the on-disk copy. You may call **audio**
as soon as the attachment has a local path; you do **not** need to wait for OSS upload.
**video** may use **remoteUrl** and does **not** hear the audio track.

### User intent → mode

| User wants | mode | workflow |
|------------|------|----------|
| What was said / transcript / 说了什么 / 逐字稿 / 语音转文字 | **audio** | One call on the video **ref**; uses **local file**, not OSS |
| Scenes / UI / demo steps / 画面 / 演示流程 | **video** | One call; wait for OSS **remoteUrl** when attached from Composer |
| Both speech and visuals | **audio** + **video** | **Two calls**, same **ref**; merge in your reply |
| On-screen text only (no speech needed) | **video** | Put OCR / UI focus in **goal** |

### Do not

- Use **video** when the user needs **spoken content** — vision cannot hear audio.
- Use **audio** when the user only needs **what is visible** on screen.
- Infer or guess a transcript from **video** mode output.

### Speech + visuals together (two-call flow)

When the user asks for both what is **said** and what is **shown**:

1. **Call 1 — `mode=audio`**: **goal** = transcribe or summarize speech per user request.
2. **Call 2 — `mode=video`**: **goal** = describe scenes, UI, actions, visible text.
3. Combine both tool results in the user-facing answer.

For long videos:

- **audio**: no time-window parameters — put segment focus in **goal**, or transcribe the
  full attachment.
- **video** (OSS native): put segment focus in **goal**; the host sends the full file.
- **video** (ffmpeg fallback): split calls with **timeStartSec** / **timeEndSec** when the
  user named a segment.

## Image directory

- **ref** may be a folder path (`localPath` or user path) when **mode=image**.
- Lists **non-recursive** image files (png/jpg/jpeg/gif/webp/bmp/heic/heif), sorted by name.
- **Default:** images **1–200** when the user did not name a range.
- **User named a range:** set **imageStart** / **imageEnd** (1-based index).
- **Max 200 images per call** — split across multiple calls for larger folders.
- Tool output includes a **scope notice** with index range and **total image count**.

## PDF pages

- **Default (user did not name pages):** host extracts **pages 1–10** only.
- **User named pages:** set **pageStart** / **pageEnd** to match their request.
- **Max 10 pages per call** — for larger ranges, call multiple times with different windows.
- Tool output includes a **scope notice** with processed pages and **total page count**.

## Video sampling

- **Primary (DashScope, `mode=video` only):** Composer videos use **`remoteUrl`**
  (HTTPS OSS URL) → native **`video_url`** + **`fps`**. Sends the **full** video;
  put time/segment focus in **goal** — **timeStartSec** / **timeEndSec** do **not**
  trim the native path.
- **Upload:** wait for OSS upload to finish before **`mode=video`** on Composer attachments.
  **`mode=audio`** on the same video does **not** require OSS.
- **Large files (>500 MB):** cannot upload directly; after user confirms, host compresses to **≤500 MB** then uploads (frame rate / resolution may drop).
- **Fallback (`mode=video`):** no `remoteUrl` or native API failure → ffmpeg JPEG frames + vision model.
- **Time window (`mode=video`, fallback only):** **timeStartSec** / **timeEndSec** / **framesPerSecond**
  trim and sample frames on the ffmpeg path (see Parameters).
- Tool output includes scope notice with window, **total duration**, and input mode.

## Other limits

| Kind | Host limit | Agent strategy |
|------|------------|----------------|
| PDF text | ~256 KiB per call; ~120k chars in goal-focused pass | Split by **pageStart/pageEnd** |
| PDF scan | 10 page images per call; 6 MB/page JPEG | Split batches; set pages only when user asked |
| Video (DashScope) | Native **video_url** + **fps** via **`remoteUrl`** (HTTPS); full file | Segment focus in **goal**, not **timeStartSec** |
| Video fallback | ffmpeg JPEG frames, max **200**/call; **timeStartSec** / **timeEndSec** apply | No `remoteUrl` or native API failure |
| Video upload | OSS at attach; **>500 MB** → confirm then host compress to **≤500 MB**; **≤5 GB** PutObject ceiling | Cancel → no attach |
| IM inbound video | Same OSS path; **>500 MB** auto-compress (no prompt); non-video IM media **30 MB** | OSS optional; local fallback |
| Image (single file) | Resized to ≤6 MB | One file per call |
| Image directory | Max **200** images/call; non-recursive | Split by **imageStart/imageEnd** |
| Audio / video → speech | **mode=audio** on video **ref**; local file + ffmpeg extract | No **timeStartSec**; segment focus in **goal** |

Manifest includes **sizeBytes** — use it to warn when a file is large.

For third-party Skill scripts that need a filesystem path, use **localPath** from the manifest.

Do **not** call when the user only sent attachments without stating what to do — ask first.

Re-run with the same **ref** and updated parameters when the user wants a different scope.
