/** Visual tone for attachment type badges (composer / message chips). */
export type AttachmentFileTone =
  | 'sheet'
  | 'word'
  | 'slides'
  | 'pdf'
  | 'zip'
  | 'code'
  | 'text'
  | 'file'

export type AttachmentFileBadge = {
  /** Short label shown on the badge (e.g. XLS, PDF). */
  label: string
  tone: AttachmentFileTone
}

/** Extension from a file name (lowercased, no dot). */
export function attachmentFileExtension(fileName: string | null | undefined): string {
  const name = (fileName ?? '').trim()
  if (!name) return ''
  const base = name.split(/[/\\]/).pop() ?? name
  const i = base.lastIndexOf('.')
  if (i <= 0 || i === base.length - 1) return ''
  return base.slice(i + 1).toLowerCase()
}

function badge(label: string, tone: AttachmentFileTone): AttachmentFileBadge {
  return { label, tone }
}

/**
 * Familiar file-type badge for non-preview attachment chips.
 * Prefer extension; fall back to MIME when the name has no suffix.
 */
export function attachmentFileBadge(
  fileName: string | null | undefined,
  mimeType?: string | null
): AttachmentFileBadge {
  const ext = attachmentFileExtension(fileName)
  if (ext) {
    if (['xlsx', 'xls', 'xlsm', 'ods'].includes(ext)) return badge('XLS', 'sheet')
    if (['csv', 'tsv'].includes(ext)) return badge('CSV', 'sheet')
    if (['doc', 'docx', 'odt', 'rtf'].includes(ext)) return badge('DOC', 'word')
    if (['ppt', 'pptx', 'odp', 'key'].includes(ext)) return badge('PPT', 'slides')
    if (ext === 'pdf') return badge('PDF', 'pdf')
    if (['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz'].includes(ext)) {
      return badge(ext === 'rar' ? 'RAR' : ext === '7z' ? '7Z' : 'ZIP', 'zip')
    }
    if (ext === 'json') return badge('JSON', 'code')
    if (['md', 'markdown'].includes(ext)) return badge('MD', 'text')
    if (['txt', 'log', 'text'].includes(ext)) return badge('TXT', 'text')
    if (
      [
        'js',
        'jsx',
        'ts',
        'tsx',
        'mjs',
        'cjs',
        'py',
        'rs',
        'go',
        'java',
        'kt',
        'c',
        'cc',
        'cpp',
        'h',
        'hpp',
        'cs',
        'rb',
        'php',
        'swift',
        'css',
        'scss',
        'less',
        'html',
        'htm',
        'vue',
        'svelte',
        'xml',
        'yaml',
        'yml',
        'toml',
        'sh',
        'bash',
        'zsh',
        'ps1',
        'sql',
        'r',
        'm'
      ].includes(ext)
    ) {
      const label = ext.length <= 4 ? ext.toUpperCase() : 'CODE'
      return badge(label, 'code')
    }
    if (ext.length <= 4) return badge(ext.toUpperCase(), 'file')
  }

  const mime = (mimeType ?? '').trim().toLowerCase()
  if (mime) {
    if (
      mime.includes('spreadsheet') ||
      mime.includes('excel') ||
      mime === 'text/csv'
    ) {
      return badge(mime === 'text/csv' ? 'CSV' : 'XLS', 'sheet')
    }
    if (mime.includes('presentation') || mime.includes('powerpoint')) {
      return badge('PPT', 'slides')
    }
    if (mime === 'application/pdf') return badge('PDF', 'pdf')
    if (
      mime.includes('zip') ||
      mime.includes('compressed') ||
      mime.includes('tar') ||
      mime.includes('gzip')
    ) {
      return badge('ZIP', 'zip')
    }
    if (mime === 'application/json' || mime.endsWith('+json')) {
      return badge('JSON', 'code')
    }
    if (mime.includes('word') || mime.includes('msword') || mime.includes('document')) {
      return badge('DOC', 'word')
    }
    if (mime.startsWith('text/')) return badge('TXT', 'text')
  }

  return badge('FILE', 'file')
}
