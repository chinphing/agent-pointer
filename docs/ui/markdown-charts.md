# Markdown charts (Chart.js)

Chat (and markdown file preview) can render interactive charts from a fenced JSON block. The model emits configuration only; the client renders with a **local** Chart.js build (no CDN, no arbitrary HTML/`script`).

## Fence format

Language: `chartjs` or `chart`.

Body: one JSON object with Chart.js fields:

- `type` (required) — e.g. `bar`, `line`, `pie`, `doughnut`, `radar`
- `data` (required)
- `pointerPalette` (optional) — host-only flag (stripped before Chart.js / IM raster). Default / omitted: host soft series palette overrides model colors. Set `false` only when the user explicitly requested custom series colors; then `borderColor` / `backgroundColor` / `fill.above|below` are kept.
- `options` (optional) — JSON only; **no functions** and no function-looking strings (e.g. `"ctx => …"` in `segment` / `ticks.callback`). The host strips those and may dash segments below zero in the series soft color (no alert red; skipped when `pointerPalette: false`). `plugins.annotation` is not bundled and is ignored.

Example:

````md
```chartjs
{
  "type": "bar",
  "data": {
    "labels": ["A", "B"],
    "datasets": [{ "data": [3, 5], "backgroundColor": ["#378ADD", "#0F6E56"] }]
  },
  "options": { "indexAxis": "y" }
}
```
````

## UI

- Card chrome matches GFM tables / code blocks (`--card`, rounded border).
- Toolbar (always visible after render): copy JSON, export PNG (desktop: Save dialog; web: browser download), toggle source.
  PNG export composites the chart onto the card background so it matches the on-screen look (raw canvas is transparent).
- Theme: axis/legend colors follow CSS variables (`--foreground`, `--muted`, `--border`).
- Series colors: by default the host applies a soft coordinated palette of 8 hues (blue → peach → teal → sand → sage → slate → mauve → olive; cycles if more series), overriding model neon colors. With root `"pointerPalette": false`, model series colors are kept (user-requested).
- **LLM context (API-only):** when building the next model request, assistant messages that contain chart fences get an appended `<!-- pointer-chart-render -->` block listing the host-applied (or custom) series colors. This is not stored in `msg.content` and is not shown in the UI — same pattern as delivered-attachment manifests.
- Hover tooltips are enabled (`interaction.mode: index`). Do **not** force CSS width/height on the `<canvas>` — that breaks hit-testing.
- Plot area height is fixed (`360px` inner box); `maintainAspectRatio` is forced off so model `aspectRatio` cannot flatten the chart.
- Cartesian series are flattened to primitive number arrays (`parseFloat`); `parsing: false` object points are avoided because they mis-scale on macOS WKWebView.
- Theme paints use resolved `rgb()`/`rgba()` (not `hsl()`) for WebKit canvas reliability; animations are disabled.
- Oversized `scales.*.min` / `max` / `suggested*` (large empty headroom above the series) are stripped so data is not glued to the floor.
- **Streaming:** do not mount Chart.js while the assistant turn is still streaming; show “图表生成中…” until the turn settles, then render once with complete fence JSON. While streaming, `parseMarkdown(..., { streamingCharts: true })` collapses every `chartjs` / `chart` fence to a **fixed** pending host so throttled `v-html` updates do not flash the card on each token.
- Invalid / incomplete JSON shows a pending or error state inside the same card.

## vs GFM tables

- **Tables**: precise values, many columns, copy-friendly detail.
- **Charts**: comparisons and trends.

HTML tables with column widths / status colors: see [markdown-media-boundaries.md](markdown-media-boundaries.md).

## IM channels (Feishu / DingTalk / WeCom / Weixin)

Platforms cannot run interactive Chart.js. On outbound delivery the host:

1. Finds `chartjs` / `chart` fences in the assistant reply
2. **Normalizes** the JSON to match App styling (soft blue/peach palette unless `pointerPalette: false`, white card background, strip function-looking strings / `annotation`)
3. **Multi Y-axis (`y1` / `y2` / …):** `fulgur-chart` cannot draw secondary axes. Host linearly remaps each non-primary value axis onto `y` so **curve shapes** match App Chart.js; legend notes `y1 尺度…` / `y2 尺度…`. Secondary tick labels are not drawn.
4. Renders to a PNG (`fulgur-chart`, Chart.js–compatible subset) using a **system CJK font** (not fulgur’s bundled Noto Sans JP). Candidates differ by OS (e.g. Hiragino Sans GB / YaHei / Noto CJK). Override with env `POINTER_IM_CHART_FONT=/path/to/font.ttf`.
5. Replaces the fence with `MEDIA:<absolute-path>` under `{app_data}/generated-media/im-charts/`
6. Sends caption text + image attachment via the existing IM media pipeline

Render failures (including missing system CJK font) keep the original fence (JSON fallback) and log a warning. Cache key includes a style version + font identity + normalized JSON so palette/font updates invalidate old PNGs.

Desktop/Web chat is unchanged (interactive canvas; Chart.js uses its default Latin stack with OS CJK fallback). Negative-segment dashes (same series soft color) are App-only; IM gets the same colors without per-segment dash.

Model prompt (shared inject): [`crates/pointer-core/src/agents/_shared/CHARTS.md`](../../crates/pointer-core/src/agents/_shared/CHARTS.md) — see [`../agents/reply-media-prompts.md`](../agents/reply-media-prompts.md).

Implementation: [`src/lib/markdownConfig.ts`](../../src/lib/markdownConfig.ts), [`src/lib/markdownChart.ts`](../../src/lib/markdownChart.ts), [`src/composables/useMarkdownCharts.ts`](../../src/composables/useMarkdownCharts.ts), [`crates/pointer-channels/src/chart_outbound.rs`](../../crates/pointer-channels/src/chart_outbound.rs).
