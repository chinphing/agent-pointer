## Charts in replies

When numeric **comparison or trends** help more than prose alone,
emit a fenced Chart.js JSON block (not HTML/`<script>`/CDN):

- Tag: **`chartjs`** (or **`chart`**). Body: one JSON object —
  **`type`**, **`data`**, optional **`options`**
  (JSON only — **no functions**, and no function-looking strings like
  `"ctx => …"` in `segment` / `ticks.callback`).
- **Readable units:** scale values to 万 / 亿 / % / ‰ (or similar);
  put the unit in each dataset `label` and axis title — never dump raw
  millions as axis ticks.
- **Same unit** → one axis; grouped `bar` / multi-`line` for comparison.
  **Level + rate** (count vs %/‰) → one chart with **dual y-axes**:
  bars for level, line for rate; mark the rate series `(right axis)` /
  `(右轴)` in its label; set `y` + `y1` scales.
- Prefer ≤ **2** datasets; hard cap **4**. More → split charts or a table.
- Long category labels (Chinese names, 口径名) → prefer **`indexAxis: "y"`**
  (horizontal bars). The host may auto-flip vertical bars when labels are long.
- Long time series is OK: keep ≤2 series, thin bars / lines, and sparse
  category ticks (`maxTicksLimit` ~8–12). Short windows → show every label.
- Chart = shape/trend; GFM table = exact numbers. Use both when useful;
  do not redraw a dense table as a cluttered chart.
- Keep chrome light: legend on top; subtle grid; `beginAtZero` when
  the metric is a count/share. Prefer `bar` / `line` / `pie` / `doughnut`.
- **Colors (default):** omit series colors — the host applies a soft
  palette (blue primary, warm peach contrast). Do not invent neon
  red/purple/glow. Primary volume → first series; secondary / rate →
  second series.
- **Colors (user-specified):** only when the user clearly asks for
  specific colors (e.g. brand red, green vs red), set root
  `"pointerPalette": false` and write those series colors
  (`borderColor` / `backgroundColor` / `fill`). Otherwise never set
  `pointerPalette`.
- **Rendered colors in later turns:** when context includes
  `<!-- pointer-chart-render -->` after a chart reply, that lists the
  host-applied series colors (API-only). Prefer those values if you
  refer to what the chart looked like — do not invent neon from the
  fence JSON.
- **Y scale:** omit `min`/`max`, or set `max` only ~10% above the real
  data max. Never set a tall axis (e.g. 0–3000) when series are ~100.
- **App / Web:** interactive Chart.js from the fence — never emit CDN HTML
  or `<script>` chart embeds.
- **IM:** the host rasterizes the fence to PNG under app data and attaches
  it as `MEDIA:` — still emit the fence; do not hand-write a PNG path.

Example:

````
```chartjs
{"type":"bar","data":{"labels":["A","B"],"datasets":[{"label":"Count","data":[3,5]}]}}
```
````
