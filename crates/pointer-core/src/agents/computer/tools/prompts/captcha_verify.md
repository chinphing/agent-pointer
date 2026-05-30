### captcha_verify

Automate CAPTCHA using the current screenshot. Choose `type`, `click`, or `drag` from what is visible on screen.

Use it directly. When a CAPTCHA is visible, call `captcha_verify` in that turn. Do not call screen-reading tools first only to understand the challenge.
Use native tool calling for this action.
Do not encode tool calls as text envelopes.

Before calling `captcha_verify`,
analyze the prompt in reasoning and include:
- Captcha Prompt Quote
- Key Verb Identified
- Selected Method & Justification

Verb-to-action mapping:
- `drag` — wording like drag, slide, puzzle, move; UI shows a slider, puzzle piece, or single drag control.
- `click` — wording like click, select, choose, check, click in order; targets are inside the CAPTCHA image, including tiles, icons, objects, visible characters, or Chinese text.
- `type` — wording like input, type, answer, fill; UI shows characters or a simple question plus a text input.

Parameters:
- `goal` required.
- `action` required: `type`, `click`, or `drag`.
- `method` optional backward-compatible alias of `action`.
- `index_captcha_area` required: index of the entire CAPTCHA region, including instructions.
- `index_input_area` required for `type`: index of the answer input.
- `remark` required: short instruction text, e.g. `Select all images with traffic lights`.
- `is_slider` optional for `drag`: true for slider CAPTCHAs.
- `index_slider_arrow` optional for `drag` with `is_slider=true`: index of the real draggable slider arrow / handle. Alias: `index_slider_handle`.

For slider CAPTCHAs, recognition still uses `index_captcha_area`. If `index_slider_arrow` is provided, drag starts from that arrow / handle and keeps the original relative offset. Before choosing it, judge the handle by relative position, shape, and color/style. A strong default cue is a right-pointing arrow near the lower-left of the sliding image on the same horizontal line as the CAPTCHA prompt text.

If the CAPTCHA is not visible yet, trigger it first and use `captcha_verify` on the next turn. Do not downgrade a visible CAPTCHA challenge to plain mouse actions just because the prompt asks to click text inside the image.
