---
schema:
  type: object
  properties:
    prompt:
      type: string
      description: "Scene / motion description"
    size:
      type: string
      description: "HappyHorse 720P/1080P; Wan 1280*720/1920*1080; Seedance 720p/1080p/2K"
    durationSeconds:
      type: integer
      description: "Video duration in seconds (HappyHorse 3–15s, Wan 2–15s, Seedance 4–15s)"
    image:
      type: string
      description: "First-frame reference — http(s), data URL, or local path"
    audio:
      type: boolean
      description: "Enable generated audio when supported (Seedance 2.0 defaults on)"
  required:
    - prompt
  additionalProperties: false
---

Generate short videos from text (and optional first-frame image). **Provider and model come from user settings** (`mediaModelOverrides.videoGeneration`) — do not pass `model`.

## When to use

- User asks for a video clip, animation, or motion from an image.
- Long-running: wait for tool result before replying again.

## Providers (configure in settings)

| Provider | Example model | Notes |
|----------|---------------|-------|
| Qwen | `happyhorse-1.0-t2v` | 文生视频; with `image`, host auto-uses i2v variant |
| Doubao | `doubao-seedance-2-0-fast-260128` | Seedance 2.0 极速/轻量（默认）；标准版 `doubao-seedance-2-0-260128` 可在设置中切换 |

Change provider/model in **Settings → 视频生成**, not in tool args.

## Parameters

- `prompt` (required): Scene / motion description.
- `size` (optional): HappyHorse `720P` / `1080P`; Seedance 2.0 `720p` / `1080p` / `2K`.
- `durationSeconds` (optional): HappyHorse 3–15s; Seedance 2.0 4–15s.
- `image` (optional): First-frame reference — `http(s)` URL, `data:` URL, or **local path** (`~/…`, absolute, cwd-relative).
- `audio` (optional): Enable generated audio when supported (Seedance 2.0 defaults on).

## Output

Returns `MEDIA:<local-path>` for the generated MP4. This is a **final-reply** tool — on success the host delivers the video automatically (see **Delivering local media in chat** in shared system rules).

Do **not** claim local `image` paths are unsupported.

## Billing

Video is billed **per output second** (not LLM tokens):

- **HappyHorse** (DashScope): official list price 720P **0.9 CNY/s**, 1080P **1.6 CNY/s**; API returns `usage.duration` for billing.
- **Seedance 2.0** (Volcengine): billed by generated video length (~**1 CNY/s** at 1080p per official guidance); use task `duration` when present.

Usage records use model key suffix `@per-sec`; `unit_count` = billable seconds.
