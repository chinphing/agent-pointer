---
schema:
  type: object
  properties:
    refs:
      type: array
      items:
        oneOf:
          - type: string
          - type: object
            properties:
              attachmentId:
                type: string
            required:
              - attachmentId
            additionalProperties: false
      minItems: 1
      description: >
        Prefer {attachmentId: "..."} whenever the current conversation's attachment
        manifest provides one. Otherwise use the manifest ref, then localPath; a user's
        explicitly typed full path is also accepted. Do not invent pointer-media:// values.
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
        Paraphrase from the user's latest message — do not call with only refs/mode.
    context:
      type: string
      description: >
        Optional extra background: prior turns, file nickname, section/page hints,
        or constraints not already in goal.
    pageStart:
      type: integer
      minimum: 1
      description: >
        Optional 1-based start (PDF page or image-dir index). Omit with pageEnd unless
        the user gave a **numeric** range — never guess page count; "所有页/全文" → omit
        (host defaults, clamped to file length).
    pageEnd:
      type: integer
      minimum: 1
      description: >
        Optional 1-based end (inclusive). Pair with pageStart only for numeric ranges.
        PDF max 10 pages/call; image directory max 200/call.
  required:
    - refs
    - mode
    - goal
  additionalProperties: false
---

Understand image, video, audio, or PDF files on demand via host-managed models.

## Parameters

- **refs** + **mode** — when the current conversation manifest provides
  **attachmentId**, pass `{attachmentId: "..."}`. Otherwise fall back to manifest **ref**,
  then **localPath**; a user's explicitly typed full path is also accepted.
  Never construct `pointer-media://` + filename yourself.
  Image mode: one directory path allowed. Other modes: exactly one ref.
- **goal** (required) — user's analysis goal in their language.
- **context** (optional) — extra thread background not already in **goal**.

### refs count by mode

| mode | refs |
|------|------|
| **image** | 1–200 refs; multi-ref for compare/batch in one call |
| **video** / **audio** / **pdf** | **exactly one** ref (single-element array) |

## Modes

- **image** / **video**: vision model (frame sampling). **video** = scenes, UI, actions,
  on-screen text (host may use OSS `video_url` or ffmpeg frames).
  **Does not read the audio track** — no transcript, no spoken-word analysis.
- **audio**: ASR model. **Speech / transcript** from an audio **or video** attachment.
  Pass the attachment in **refs**; for video files the host **extracts the audio
  track** then runs ASR. **Does not see the picture.**
- **pdf**: **scanned-PDF fallback only** — Pdfium renders each page to JPEG, then vision model. Use the **pdf** skill + `terminal` first for text-native PDFs.

## Video attachments: vision vs speech

**video** mode analyzes **frames only**. Spoken words, music, and sound effects are
**not** available to the vision model — even when the file is a normal `.mp4` with sound.

**audio** mode analyzes **sound only**. For a **video** ref the host runs this pipeline:

1. Resolve the attachment from the manifest (**refs[0]**) and read the **local file**.
2. **Extract the audio track** from the video file (ffmpeg).
3. Run speech-to-text (ASR).
4. Return text shaped by **goal** (verbatim transcript, summary, speaker labels, etc.).

**audio** does **not** use OSS **remoteUrl** — only the on-disk copy. You may call **audio**
as soon as the attachment has a local path; you do **not** need to wait for OSS upload.
**video** may use **remoteUrl** and does **not** hear the audio track.

### User intent → mode

| User wants | mode | workflow |
|------------|------|----------|
| What was said / transcript / 说了什么 / 逐字稿 / 语音转文字 | **audio** | One call; **refs** with one video/audio ref; uses **local file**, not OSS |
| Scenes / UI / demo steps / 画面 / 演示流程 | **video** | One call; wait for OSS **remoteUrl** when attached from Composer |
| Both speech and visuals | **audio** + **video** | **Two calls**, same ref in **refs**; merge in your reply |
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

- **audio**: put segment focus in **goal**, or transcribe the full attachment.
- **video** (OSS native): put segment focus in **goal**; the host sends the full file at **1 fps**.
- **video** (ffmpeg fallback): host processes the **first segment** (up to 200s at 1 fps); put later segments in **goal** and call again if needed.

## Image directory

- **refs** with one folder path when **mode=image**.
- Lists **non-recursive** image files (png/jpg/jpeg/gif/webp/bmp/heic/heif), sorted by name.
- **Default:** images **1–200** when pageStart/pageEnd omitted.
- **Max 200 images per call** — split across multiple calls for larger folders.
- Tool output includes a **scope notice** with index range and **total image count**.

## Multiple image attachments

- **mode=image** with **refs** (2+ elements): compare or summarize in one vision call.
- **Max 200 refs per call** — same cap as image directory batches.
- For unrelated images with different goals, prefer separate calls with one ref each.
- Tool output includes a **scope notice** with the ref count processed.

## PDF pages

- **Default:** use the **pdf** skill + `terminal` on **`localPath`** to extract text.
- **`mode=pdf` here:** only when extraction is empty or unusable (scanned PDF).
- **Max 10 pages per call** — split with pageStart/pageEnd when the user gave a numeric range beyond that.

## Video sampling

- **Primary (DashScope, `mode=video` only):** Composer videos use **`remoteUrl`**
  (HTTPS OSS URL) → native **`video_url`** at **1 fps**. Sends the **full** video;
  put time/segment focus in **goal**.
- **Upload:** wait for OSS upload to finish before **`mode=video`** on Composer attachments.
  **`mode=audio`** on the same video does **not** require OSS.
- **Large files (>500 MB):** cannot upload directly; after user confirms, host compresses to **≤500 MB** then uploads (frame rate / resolution may drop).
- **Fallback (`mode=video`):** no `remoteUrl` or native API failure → ffmpeg JPEG frames + vision model (first **200s** at **1 fps** by default).
- Tool output includes scope notice with window, **total duration**, and input mode.

## Other limits

| Kind | Host limit | Agent strategy |
|------|------------|----------------|
| PDF (scanned fallback) | Pdfium page render → vision model | Use **pdf** skill first; split by **pageStart/pageEnd** |
| PDF page images (`mode=pdf`) | 10 pages/call; 6 MB/page JPEG | Only after skill detects scan; split batches when user asked |
| Video (DashScope) | Native **video_url** at **1 fps** via **`remoteUrl`** (HTTPS); full file | Segment focus in **goal** |
| Video fallback | ffmpeg JPEG frames, max **200**/call; first segment at **1 fps** | No `remoteUrl` or native API failure |
| Video upload | OSS at attach; **>500 MB** → confirm then host compress to **≤500 MB**; **≤5 GB** PutObject ceiling | Cancel → no attach |
| IM inbound video | Same OSS path; **>500 MB** auto-compress (no prompt); non-video IM media **30 MB** | OSS optional; local fallback |
| Image (single file) | Resized to ≤6 MB | **refs** with one element |
| Image (multi ref) | Max **200** refs/call | **refs** with 2+ elements |
| Image directory | Max **200** images/call; non-recursive | **refs** with one folder + **pageStart/pageEnd** |
| Audio / video → speech | **mode=audio** on video ref; local file + ffmpeg extract | **refs** with one element; segment focus in **goal** |

Manifest includes **sizeBytes** — use it to warn when a file is large.

For third-party Skill scripts that need a filesystem path, use **localPath** from the manifest.

Do **not** call when the user only sent attachments without stating what to do — ask first.

Re-run with the same **refs** and updated parameters when the user wants a different scope.
