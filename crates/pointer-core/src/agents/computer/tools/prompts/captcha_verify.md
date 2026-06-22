### captcha_verify_type / captcha_verify_click / captcha_verify_drag

Flat CAPTCHA tools — tool name is the action. No `method`.

Use on a **new or unsolved** challenge only. Do not screen-read first.

Before calling, in reasoning: **Captcha Prompt Quote**, **Key Verb**, **Variant & why**.

Verb → tool:
- **`captcha_verify_drag`** — drag / slide / puzzle / move; slider or puzzle piece.
- **`captcha_verify_click`** — click / select / choose / in order; targets inside the image.
- **`captcha_verify_type`** — input / type / answer / fill; text field + question.

Args: `goal`; `index_captcha_area` (whole CAPTCHA); `index_input_area` (type only); optional `remark`; drag: optional `is_slider`, `index_slider_arrow` / `index_slider_handle`.

Run **Index parameters** chain per overlay arg before calling.

Slider: crop via `index_captcha_area`; optional `index_slider_arrow` = real handle (often `→` lower-left of slide, same row as prompt).

**Two-phase:** (1) `captcha_verify_*` once — solve inside the image. (2) If a separate **Confirm / Verify / Submit** button exists, click with **`mouse_*`**, not captcha again. Panel may look unchanged until (2). **`action_verify`** after (2), or after (1) if no submit button (e.g. release-to-validate slider). Repeat captcha only on **new puzzle** or **failed retry**. In-challenge targets stay on captcha tools; submit button uses `mouse_*`.

If CAPTCHA hidden, reveal with mouse first, then captcha on next turn.
