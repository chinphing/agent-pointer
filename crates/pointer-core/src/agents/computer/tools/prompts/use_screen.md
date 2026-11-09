### use_screen

Move the captured display to another screen.

`[CUR_SCREEN]` captures **one display at a time**. When the app you must work
with sits on a different display, the frame in front of you does not contain it.
Do not go hunting with `terminal`/`screencapture` — switch the capture instead.

Optional:
- **`display`**: the display id **exactly as `launch_app` reported it** (for
  example `3`). Take it from that reply; do not invent or re-derive it. Omit to
  keep the current display (the reply lists every display and its id).

**`action` field:** say which app or display you want to see and why.

The next `[CUR_SCREEN]` frame comes from the chosen display.
