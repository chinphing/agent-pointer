# chat-layout-probe

Dev-only probe for the **"composer height change → whole transcript layout"**
cascade. Not part of the app bundle: `tools/**` is outside `tsconfig.json`'s
`include` and outside Vite's entry graph, so it never reaches `dist/`.

It reproduces the exact box tree that carries the cascade, using the **real**
stylesheet (`src/styles/globals.css`), the **real** virtualizer
(`@tanstack/vue-virtual`), and the app's own modules
(`lib/messageVirtualization`, `lib/virtualOffsetObservation`,
`lib/composerTextareaResize`) — so a keystroke here pays the same layout and
script costs the app pays, without booting the chat store.

```
ChatView root  div.flex-1.flex.flex-col.min-h-0
  main pane    div.chat-main.flex-1.overflow-hidden.relative
    …          div.flex.h-full.min-h-0.flex-col > div.relative.min-h-0.flex-1
      list     div.relative.h-full.min-h-0
        scroll div.chat-scroll-area.h-full.overflow-y-auto.chat-shell.pb-6
          wrap div.flex.min-h-full.w-full.flex-col          ← percentage height
            col  div.chat-column.shrink-0 (height = totalSize)
                 div.chat-virtual-row.absolute… (contain: layout)
  composer     div.chat-shell.shell-chat.shrink-0.pt-2.pb-5
                 textarea.composer-textarea (field-sizing: content)
```

## Run

```bash
npm run dev                       # another shell; vite on :1420
node tools/chat-layout-probe/run.mjs --variant baseline
node tools/chat-layout-probe/run.mjs --variant overlay --out /tmp/overlay.json
```

The driver launches headless Chrome over CDP, types real keystrokes, and reports
per keystroke: `Performance.getMetrics` deltas (layout / style / script / task),
plus the probe's own counters (scroller-ResizeObserver callbacks,
compensations, scroll events, scroll passes, virtualizer range changes) and the
`composerResize` frame's cost. Phases: `soft` (typing; height changes only at
wrap boundaries), `grow` (a newline per step: height changes every step),
`pulse` (newline + backspace), `delete` (backspace).

## Variants

| `--variant` | structure |
|---|---|
| `baseline` | as shipped |
| `nominheight` | transcript wrapper without `min-h-full` |
| `containcontent` | `contain: layout` on the transcript wrapper |
| `containscroller` | `contain: layout` on the scroller |
| `overlay` | composer absolutely positioned; wrapper `padding-bottom` tracks it |
| `overlay_scrollpad` | same, but the inset is on the scroller |
| `overlay_nominheight` | `overlay` + no `min-h-full` |
| `isolated` | textarea out of flow inside a JS-height-managed box |
| `flexend` | scroller is a flex column; wrapper bottom-aligned with `margin-top: auto` |

Flags: `--ro 0` (no scroller ResizeObserver), `--fallback 1` (JS auto-grow path),
`--skipcomp 1` (the compensation's own scroll event does not schedule the scroll
pass), `--rows`, `--words`, `--soft`, `--grow`, `--pulses`, `--deletes`,
`--settle`, `--width`, `--height`, `--out`.

## Reading the numbers

`layCnt` / `layMs` are the engine's own layout accounting
(`Performance.getMetrics`), so they are comparable across variants but are
**Blink** numbers: the reported jank is on WebKit (macOS Tauri), where the
`sample` shows `RenderBlock::layout` / `RenderFlexibleBox::layoutBlock`. Treat
this probe as the *mechanism* proof and the variant comparison; the decisive
A/B is the render-perf HUD on the real machine.
