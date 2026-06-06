/** Parse `command` from terminal tool `arguments` JSON (or raw fallback). */
export function parseTerminalCommandFromArgs(argumentsJson: string): string {
  const text = argumentsJson?.trim()
  if (!text) return ''
  try {
    const parsed = JSON.parse(text) as { command?: unknown }
    if (typeof parsed.command === 'string') return parsed.command.trim()
  } catch {
    return text
  }
  return ''
}
