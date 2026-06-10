---
schema:
  type: object
  properties:
    prompt:
      type: string
      description: What to generate
    model:
      type: string
      description: "e.g. wan2.7-image-pro, qwen-image-2.0-pro, doubao-seedream-5-0-lite-260128"
    size:
      type: string
      description: "1K, 2K, 4K or provider-specific (Seedream: 2K)"
    image:
      type: string
      description: "Reference image URL or local path for edit / image-to-image"
    count:
      type: integer
      description: "Number of images (1–4, default 1)"
  required:
    - prompt
  additionalProperties: false
---

Generate images from text (and optional reference image) using configured Qwen Wan / Qwen-Image or Doubao Seedream models.

## When to use

- User asks to create, draw, or edit an image.
- You need a visual asset to send via IM (`MEDIA:` or `channel_message`).

## Providers (configure API keys in settings)

| Provider | Default model | Notes |
|----------|---------------|-------|
| Qwen (DashScope) | `wan2.7-image-pro` | Unified gen/edit; 2K/4K; alt `qwen-image-2.0-pro` for text-heavy posters |
| Doubao (Volcengine Ark) | `doubao-seedream-5-0-lite-260128` | Seedream 5.0 Lite; alt `doubao-seedream-4-5-251128` |

Override via `mediaModelOverrides.imageGeneration` in settings or tool `model` arg.

## Parameters

- `prompt` (required): What to generate.
- `model` (optional): e.g. `wan2.7-image-pro`, `qwen-image-2.0-pro`, `doubao-seedream-5-0-lite-260128`.
- `size` (optional): `1K`, `2K`, `4K`, or provider-specific (Seedream: `2K`).
- `image` (optional): Reference image URL or local path for edit / image-to-image.
- `count` (optional): Number of images (1–4, default 1).

## Output

Returns `MEDIA:<local-path>` lines. Use those paths with `channel_message` or append `MEDIA:` in IM replies.

This tool is a **final reply** tool: on success the host ends the turn and delivers output in chat.

## Billing

DashScope Wan 2.7 reports `usage.total_tokens` (token-based). Seedream is billed per image; usage is recorded for platform reporting.
