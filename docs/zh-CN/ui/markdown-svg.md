# Markdown SVG 示意图

Chat (and markdown file preview) can render inline diagrams from a fenced SVG block. The model emits SVG markup only; the client sanitizes then mounts it (no arbitrary HTML pages, no scripts).

## Fence format

Language: `svg`.

Body: one self-contained `<svg>…</svg>` document (optional XML declaration before the root is stripped to the SVG root).

Example:

````md
```svg
<svg viewBox="0 0 320 120" xmlns="http://www.w3.org/2000/svg" role="img">
  <title>Request flow</title>
  <rect x="20" y="40" width="80" height="40" rx="8" fill="#E6F1FB" stroke="#185FA5"/>
  <text x="60" y="64" text-anchor="middle" font-size="12" fill="#0C447C">Start</text>
</svg>
```
````

## Security

Before mount the host:

- Caps size (`MAX_SVG_BYTES`)
- Requires a single `<svg>` root
- Strips `<script>`, `<foreignObject>`, `<iframe>`, `<embed>`, `<object>`, `<link>`, `<meta>`, `<base>`
  (Mermaid diagrams keep a sanitized `<foreignObject>` for some node labels; see [markdown-mermaid.md](markdown-mermaid.md))
- Removes `on*` event handlers
- Neutralizes `javascript:` / `vbscript:` / `data:` / remote absolute URLs in `href` / `src` / similar
- Strips dangerous `style` expressions
- Escapes a bare `&` in markup (`&amp;`) so query-string labels do not fail XML parse

Internal fragment refs (`href="#id"`) remain allowed (e.g. markers / `<use>`).

## UI

- Fence chrome matches code / Mermaid / charts (`--fence-bg` = `--shell-chat`, rounded border).
- Toolbar: copy source, export `.svg`, toggle source. Default **hidden**; show on diagram hover / focus-within (same as Mermaid / code-block copy). Stay visible while source view is active. Touch / coarse pointers keep the toolbar always visible.
- **Export:** builds a URI-encoded `data:image/svg+xml;charset=utf-8,…` URL. Desktop save must decode that payload to UTF-8 bytes (then base64 for `save_bytes_to_path`); do not treat the percent-encoded payload as base64 (PNG chart export uses `;base64,` and is unaffected).
- **Layout:** mount sizes the SVG from `viewBox` (pixel width). Narrow chat panes scroll horizontally instead of crushing labels with `max-width: 100%`. Undersized `viewBox` (content drawn past the bottom/side) is expanded from geometry attributes / `getBBox` so footers are not clipped. The frame resets prose inheritance (font-size / line-height / overflow-wrap) so `.md-body` typography does not change SVG text metrics.
- **Streaming:** while the fence is still open, show “图示生成中…”. Once the closing ` ``` ` arrives, mount immediately — do not wait for the rest of the assistant turn. Trailing prose may still be streaming. A **closed** fence that still fails sanitize shows “图示无效”, not the generating placeholder.
- **Switch / scroll:** do not parse/insert the SVG until the host is in the nearest chat or file-preview scroller (same gate as Chart.js / Mermaid). Hidden workspace tabs wait until shown.

## vs charts

| Need | Fence |
|------|--------|
| Numeric comparison / trends | `chartjs` / `chart` |
| Process / architecture / decision flow | `svg`（coder **`architecture_explain`** 场景优先强制用 SVG 总览） |

Full media boundaries (Markdown / SVG / Chart / HTML tables): [markdown-media-boundaries.md](markdown-media-boundaries.md).

## IM channels

Interactive SVG runs in App/Web. IM rasterization is not guaranteed; the model prompt asks for a short textual summary when the diagram is essential.

Implementation: [`src/lib/markdownConfig.ts`](../../../src/lib/markdownConfig.ts), [`src/lib/markdownSvg.ts`](../../../src/lib/markdownSvg.ts), [`src/composables/useMarkdownSvgs.ts`](../../../src/composables/useMarkdownSvgs.ts).
