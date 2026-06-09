Generate short videos from text (and optional first-frame image) using Qwen Wan or Doubao Seedance models.

## When to use

- User asks for a video clip, animation, or motion from an image.
- Long-running: wait for tool result before replying again.

## Providers

| Provider | Default model | Notes |
|----------|---------------|-------|
| Qwen | `wan2.7-t2v` | Audio + multi-shot; alt `wan2.6-t2v` |
| Doubao | `doubao-seedance-1-5-pro-251215` | Audio; alt `doubao-seedance-1-0-pro-250528` |

Seedance 2.0 uses million-token video billing (~1 CNY/s) but API is not generally available yet — do not pass `seedance-2.0` model ids.

## Parameters

- `prompt` (required): Scene / motion description.
- `model` (optional): Provider model id.
- `size` (optional): DashScope e.g. `1280*720`, `1920*1080`; Seedance resolution `720p` / `1080p`.
- `durationSeconds` (optional): Target length (2–15 for Wan 2.7).
- `image` (optional): First-frame image URL or path (image-to-video).
- `audio` (optional): Enable generated audio when supported.

## Output

Returns `MEDIA:<local-path>` for the generated MP4.

## Billing

Wan returns provider token usage when available. Seedance 1.x is approximated per video-second for usage records.
