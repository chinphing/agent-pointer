---
schema:
  type: object
  properties:
    ref:
      type: string
      description: >
        Media reference: pointer-media:// URI, conversation-media relative path,
        or absolute local path from the user attachment manifest.
    mode:
      type: string
      enum:
        - image
        - video
        - audio
        - pdf
      description: Understanding mode (must match file kind).
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
  required:
    - ref
    - mode
    - goal
  additionalProperties: false
---

Understand image, video, audio, or PDF files on demand via host-managed models.

## Parameters

- **ref** + **mode** — from the user attachment manifest (`pointer-media://…`).
- **goal** (required) — user's analysis goal in their language, e.g.
  "Summarize key risks in this contract", "Transcribe the meeting recording",
  "Read the error message on page 3". Include output expectations when stated.
- **context** (optional) — extra thread background not already in **goal**.

## Modes

- **image** / **video**: qwen3.5-flash (vision).
- **audio**: qwen3-asr-flash.
- **pdf**: sorted text extraction; scanned pages fall back to page-image OCR.

For third-party Skill scripts that need a filesystem path, use **localPath** from the manifest.

Do **not** call when the user only sent attachments without stating what to do — ask first.

Re-run with the same **ref** and updated **goal** when the user wants a different analysis.
