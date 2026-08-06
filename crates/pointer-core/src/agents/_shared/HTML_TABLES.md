## HTML tables in replies

When a table needs **fixed column widths** (Markdown cannot set `%` widths),
emit a fenced HTML table (App / Web host renders `html` fences as HTML):

- Tag: **`html`** (or **`htm`**). Body: one or more safe HTML fragments,
  typically a single `<table>…</table>`.
- Prefer `<colgroup><col style="width:…%">` for column widths, or
  `width` on `<th>` / `<td>` / the `<table>`.
- Keep tables simple: no `<script>`, event handlers, iframes, forms,
  or embedded SVG/MathML — use the `svg` / `chartjs` fences for diagrams
  and charts.
- Status colors via inline `style` on cells are fine (e.g. green / amber / red).
- Long cell text may use `<br>` for controlled line breaks.
- **Default:** still prefer GFM pipe tables when fixed widths are not needed
  (simpler, better IM compatibility).
- **IM:** HTML tables are unreliable — also give a short textual summary
  or a GFM table when the numbers matter outside App/Web.

Example:

````
```html
<table>
  <colgroup>
    <col style="width:8%">
    <col style="width:12%">
    <col style="width:50%">
    <col style="width:15%">
    <col style="width:15%">
  </colgroup>
  <tr>
    <th>#</th>
    <th>Name</th>
    <th>Description</th>
    <th>Amount</th>
    <th>Status</th>
  </tr>
  <tr>
    <td>1</td>
    <td>Zhang</td>
    <td>Travel to Beijing for review meeting…</td>
    <td>1,280.50</td>
    <td style="color:#1a7f37;">OK</td>
  </tr>
</table>
```
````
