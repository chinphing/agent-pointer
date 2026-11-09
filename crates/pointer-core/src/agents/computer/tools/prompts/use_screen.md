### use_screen

Switch the captured display to the one the target app lives on.

`[CUR_SCREEN]` captures **one display at a time**. When the app you were told to
work with is open on another display, the frame in front of you does not contain
it — do not go hunting with `terminal`/`screencapture`, call this instead.

Optional:
- **`app`**: the app whose display you want. Omit to follow the frontmost app.

**`action` field:** say which app (or display) you want to see and why.

The reply lists every display and marks the one capture will move to. The next
`[CUR_SCREEN]` frame comes from that display.
