---
schema:
  type: object
  properties:
    prompt:
      type: string
      description: What to generate
    size:
      type: string
      description: "1K, 2K, 4K or provider-specific (Seedream: 2K)"
    image:
      type: string
      description: "Reference — http(s) URL, data URL, or local path (~/…, absolute, cwd-relative)"
    count:
      type: integer
      description: "Number of images (1–4, default 1)"
  required:
    - prompt
  additionalProperties: false
---

Generate images from text (and optional reference image). **Provider and model come from user settings** (`mediaModelOverrides.imageGeneration`) — do not pass `model`.

## When to use

- User asks to create, draw, or edit an image.
- You need a visual asset to send via IM (`MEDIA:` or `channel_message`).

## Providers (configure in settings)

| Provider | Example model | Notes |
|----------|---------------|-------|
| Qwen (DashScope) | `wan2.7-image-pro` | Unified gen/edit; 2K/4K |
| Doubao (Volcengine Ark) | `doubao-seedream-5-0-lite-260128` | Seedream 5.0 Lite |

Change provider/model in **Settings → 图片生成**, not in tool args.

## Parameters

- `prompt` (required): What to generate.
- `size` (optional): `1K`, `2K`, `4K`, or provider-specific (Seedream: `2K`).
- `image` (optional): Reference for edit / image-to-image — `http(s)`, `data:`, or **local path** (`~/…`, absolute, cwd-relative).
- `count` (optional): Number of images (1–4, default 1).

## Output

Returns `MEDIA:<local-path>` lines. Use those paths with `channel_message` or append `MEDIA:` in IM replies.

This tool is a **final reply** tool: on success the host ends the turn and delivers output in chat.

## Billing

DashScope Wan 2.7 reports `usage.total_tokens` (token-based). Seedream is billed per image; usage is recorded for platform reporting.
