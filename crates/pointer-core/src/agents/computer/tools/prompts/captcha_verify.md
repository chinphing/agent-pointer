### captcha_verify_type / captcha_verify_click / captcha_verify_drag

Flat CAPTCHA tools — each tool name is the complete action. No `method` parameter.

Automate CAPTCHA using the current screenshot. Choose the right tool from what is visible on screen.

Use it directly. When a CAPTCHA is visible, call the right variant in that turn. Do not call screen-reading tools first only to understand the challenge.

Before calling, analyze the prompt in reasoning and include:
- Captcha Prompt Quote
- Key Verb Identified
- Selected Variant & Justification

Verb-to-tool mapping:
- **`captcha_verify_drag`** — wording like drag, slide, puzzle, move; UI shows a slider, puzzle piece, or single drag control.
- **`captcha_verify_click`** — wording like click, select, choose, check, click in order; targets are inside the CAPTCHA image, including tiles, icons, objects, visible characters, or Chinese text.
- **`captcha_verify_type`** — wording like input, type, answer, fill; UI shows characters or a simple question plus a text input.

Parameters:
- `goal` required.
- `index_captcha_area` required: index of the entire CAPTCHA region, including instructions.
- `index_input_area` required for `captcha_verify_type`: index of the answer input.
- `remark` optional: short instruction text, e.g. `Select all images with traffic lights`.
- `is_slider` optional for `captcha_verify_drag`: true for slider CAPTCHAs.
- `index_slider_arrow` optional for `captcha_verify_drag` with `is_slider=true`: index of the real draggable slider arrow / handle. Alias: `index_slider_handle`.

For tools with overlay index args, run the **Index parameters** per-parameter chain in communication before calling — one **Parameter: `<arg_name>`** block per arg.

For slider CAPTCHAs, recognition still uses `index_captcha_area`. If `index_slider_arrow` is provided, drag starts from that arrow / handle and keeps the original relative offset. Before choosing it, judge the handle by relative position, shape, and color/style. A strong default cue is a right-pointing arrow near the lower-left of the sliding image on the same horizontal line as the CAPTCHA prompt text.

If the CAPTCHA is not visible yet, trigger it first and use a captcha_verify tool on the next turn. Do not downgrade a visible CAPTCHA challenge to plain mouse actions just because the prompt asks to click text inside the image.
