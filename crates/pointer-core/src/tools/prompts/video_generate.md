---
schema:
  type: object
  properties:
    prompt:
      type: string
      description: "Scene / motion description"
    model:
      type: string
      description: "e.g. happyhorse-1.0-t2v, doubao-seedance-2-0-260128"
    size:
      type: string
      description: "HappyHorse 720P/1080P; Wan 1280*720/1920*1080; Seedance 720p/1080p/2K"
    durationSeconds:
      type: integer
      description: "Video duration in seconds (HappyHorse 3–15s, Wan 2–15s, Seedance 4–15s)"
    image:
      type: string
      description: "First-frame image URL or path for image-to-video"
    audio:
      type: boolean
      description: "Enable generated audio when supported (Seedance 2.0 defaults on)"
  required:
    - prompt
  additionalProperties: false
---

Generate short videos from text (and optional first-frame image) using Qwen **HappyHorse** or Doubao **Seedance 2.0** models.

## When to use

- User asks for a video clip, animation, or motion from an image.
- Long-running: wait for tool result before replying again.

## Providers

| Provider | Default model | Notes |
|----------|---------------|-------|
| Qwen | `happyhorse-1.0-t2v` | Native audio-video (文生视频); `happyhorse-1.0-i2v` for first-frame (图生视频) |
| Doubao | `doubao-seedance-2-0-260128` | Seedance 2.0; alt `doubao-seedance-2-0-fast-260128` |

With a first-frame `image`, HappyHorse auto-switches to `happyhorse-1.0-i2v`. Seedance 2.0 uses the same model id with `ratio: adaptive`.

## Parameters

- `prompt` (required): Scene / motion description.
- `model` (optional): Provider model id.
- `size` (optional): HappyHorse `720P` / `1080P`; Seedance 2.0 `720p` / `1080p` / `2K`.
- `durationSeconds` (optional): HappyHorse 3–15s; Seedance 2.0 4–15s.
- `image` (optional): First-frame image URL or path (image-to-video).
- `audio` (optional): Enable generated audio when supported (Seedance 2.0 defaults on).

## Output

Returns `MEDIA:<local-path>` for the generated MP4.

## Billing

Video is billed **per output second** (not LLM tokens):

- **HappyHorse** (DashScope): official list price 720P **0.9 CNY/s**, 1080P **1.6 CNY/s**; API returns `usage.duration` for billing.
- **Seedance 2.0** (Volcengine): billed by generated video length (~**1 CNY/s** at 1080p per official guidance); use task `duration` when present.

Usage records use model key suffix `@per-sec`; `unit_count` = billable seconds.
