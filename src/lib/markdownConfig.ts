import { marked } from 'marked'

// Configure marked once at module load — all importers share this instance.
marked.setOptions({ breaks: true, gfm: true })

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

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
    },
    code({ text, lang, escaped }) {
      const langString = (lang || '').match(/^\S*/)?.[0] || ''
      const code = text.replace(/\n$/, '') + '\n'
      const body = escaped ? code : escapeHtml(code)
      const langClass = langString ? ` class="language-${escapeHtml(langString)}"` : ''
      return (
        `<div class="code-block"><pre><code${langClass}>${body}</code></pre></div>\n`
      )
    },
  },
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
