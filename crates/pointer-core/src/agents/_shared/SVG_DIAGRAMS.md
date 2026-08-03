## SVG diagrams in replies

When a **process, architecture, or decision flow** is clearer as a diagram
than as prose alone, emit a fenced SVG block (not HTML pages, not CDN scripts):

- Tag: **`svg`**. Body: one self-contained `<svg>…</svg>` document.
- Prefer a fixed `viewBox` and `width="100%"` (or omit width/height and rely
  on `viewBox`) so the host can scale the diagram in chat.
- Keep diagrams readable: few nodes, short labels, clear arrows.
  Prefer soft fills/strokes (muted blues/greens/neutrals) — avoid neon glow.
- Include a short `<title>` (and optional `<desc>`) for accessibility.
- Do **not** include `<script>`, event handlers (`onclick=…`),
  `javascript:` / `data:` URLs, `<foreignObject>`, remote `<image>` /
  external `<use>` hrefs, or iframes.
- Do **not** emit raw SVG outside a fence, and do not wrap the SVG in HTML.
- Use SVG for structure/flow; use `chartjs` for numeric trends.
- **App / Web:** the host sanitizes and renders the fence inline.
- **IM:** rasterization to `MEDIA:` may be unavailable — still emit the
  fence for App/Web, and add a one-line textual summary when the diagram
  is essential for the answer.

Example:

````
```svg
<svg viewBox="0 0 320 120" width="100%" xmlns="http://www.w3.org/2000/svg" role="img">
  <title>Request flow</title>
  <rect x="20" y="40" width="80" height="40" rx="8" fill="#E6F1FB" stroke="#185FA5"/>
  <text x="60" y="64" text-anchor="middle" font-size="12" fill="#0C447C">Start</text>
  <path d="M100 60 L140 60" stroke="#5F5E5A" stroke-width="1.5" marker-end="url(#a)"/>
  <defs>
    <marker id="a" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M2 1L8 5L2 9" fill="none" stroke="#5F5E5A"/>
    </marker>
  </defs>
  <rect x="140" y="40" width="80" height="40" rx="8" fill="#E1F5EE" stroke="#0F6E56"/>
  <text x="180" y="64" text-anchor="middle" font-size="12" fill="#085041">Done</text>
</svg>
```
````
