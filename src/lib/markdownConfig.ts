import { marked } from 'marked'

// Configure marked once at module load — all importers share this instance.
marked.setOptions({ breaks: true, gfm: true })

marked.use({
  renderer: {
    table({ header, rows }) {
      const h = header.map(c => '<th>' + marked.parseInline(c.text ?? '') + '</th>').join('')
      const body = rows
        .map(r => '<tr>' + r.map(c => '<td>' + marked.parseInline(c.text ?? '') + '</td>').join('') + '</tr>')
        .join('')
      return (
        '<div class="table-wrapper"><table><thead><tr>' +
        h +
        '</tr></thead><tbody>' +
        body +
        '</tbody></table></div>'
      )
    }
  }
})

/**
 * Parse Markdown to HTML with shared configuration.
 *
 * Includes a pre-processing step that inserts a blank line after GFM tables
 * when the next line is not a pipe or whitespace, which prevents the parser
 * from swallowing the table into the following paragraph.
 */
export function parseMarkdown(src: string): string {
  if (!src.trim()) return ''
  const fixed = src.replace(/(\|[^\n]*\|\s*\n)(?=[^\s|])/g, '$1\n')
  return marked.parse(fixed) as string
}
