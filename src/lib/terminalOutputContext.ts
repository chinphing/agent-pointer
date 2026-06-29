const INPUT_CONTEXT_MAX_LINES = 10
const INPUT_CONTEXT_MAX_CHARS = 1200

/** Strip common ANSI/CSI sequences from PTY output for readable UI context. */
export function stripAnsiEscapes(text: string): string {
  return text
    .replace(/\u001b\[[0-9;?]*[ -/]*[@-~]/g, '')
    .replace(/\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g, '')
}

function tailLines(text: string, maxLines: number): string {
  const lines = text.split('\n')
  if (lines.length === 0) return ''
  const start = Math.max(0, lines.length - maxLines)
  return lines.slice(start).join('\n')
}

/** Recent terminal tail for the input modal (matches backend snippet sizing). */
export function formatTerminalOutputContext(raw: string): string | undefined {
  const cleaned = stripAnsiEscapes(raw).trim()
  if (!cleaned) return undefined
  let tail = tailLines(cleaned, INPUT_CONTEXT_MAX_LINES)
  if (tail.length > INPUT_CONTEXT_MAX_CHARS) {
    tail = tail.slice(tail.length - INPUT_CONTEXT_MAX_CHARS)
  }
  return tail || undefined
}
