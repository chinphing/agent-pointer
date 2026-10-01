export interface AskUserOption {
  label: string
  description?: string
}

export interface AskUserArgs {
  question: string
  options: AskUserOption[]
  multiSelect: boolean
}

function extractBalancedJsonArray(text: string): string | null {
  const start = text.indexOf('[')
  if (start < 0) return null
  let depth = 0
  let inString = false
  let escape = false
  for (let i = start; i < text.length; i += 1) {
    const ch = text[i]!
    if (inString) {
      if (escape) {
        escape = false
        continue
      }
      if (ch === '\\') {
        escape = true
        continue
      }
      if (ch === '"') inString = false
      continue
    }
    if (ch === '"') {
      inString = true
      continue
    }
    if (ch === '[') depth += 1
    else if (ch === ']') {
      depth -= 1
      if (depth === 0) return text.slice(start, i + 1)
    }
  }
  return null
}

/** Models sometimes send `options` as a stringified JSON array. */
function coerceOptions(options: unknown): unknown[] | null {
  if (Array.isArray(options)) return options
  if (typeof options !== 'string') return null
  const trimmed = options.trim()
  if (!trimmed) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(trimmed)
  } catch {
    const slice = extractBalancedJsonArray(trimmed)
    if (!slice) return null
    try {
      parsed = JSON.parse(slice)
    } catch {
      return null
    }
  }
  return Array.isArray(parsed) ? parsed : null
}

function optionsFromUnknown(raw: unknown): AskUserOption[] {
  const list = coerceOptions(raw)
  if (!list) return []
  return list
    .filter((item): item is { label: string; description?: string } =>
      !!item && typeof item === 'object' && typeof (item as { label?: unknown }).label === 'string'
    )
    .map(item => ({
      label: item.label.trim(),
      description: typeof item.description === 'string' && item.description.trim()
        ? item.description.trim()
        : undefined
    }))
    .filter(item => item.label)
}

export function parseAskUserArgs(raw?: string): AskUserArgs | null {
  if (!raw?.trim()) return null
  try {
    const value = JSON.parse(raw) as {
      question?: unknown
      options?: unknown
      multi_select?: unknown
    }
    if (typeof value.question !== 'string' || !value.question.trim()) return null
    const options = optionsFromUnknown(value.options)
    if (options.length < 2) return null
    return {
      question: value.question.trim(),
      options,
      multiSelect: value.multi_select === true
    }
  } catch {
    return null
  }
}

/**
 * Backend display summary when arguments are still empty:
 * question, then `1. label` or `1. label - description`.
 */
export function parseAskUserSummary(raw?: string): AskUserArgs | null {
  const lines = (raw ?? '')
    .split('\n')
    .map(line => line.trim())
    .filter(Boolean)
  if (lines.length < 3) return null
  const question = lines[0]!
  const options: AskUserOption[] = []
  for (const line of lines.slice(1)) {
    const match = /^(\d+)\.\s+(.+)$/.exec(line)
    if (!match) continue
    const body = match[2]!.trim()
    const dash = body.indexOf(' - ')
    if (dash > 0) {
      const label = body.slice(0, dash).trim()
      const description = body.slice(dash + 3).trim()
      if (label) options.push({ label, description: description || undefined })
    } else if (body) {
      options.push({ label: body })
    }
  }
  if (!question || options.length < 2) return null
  return { question, options, multiSelect: false }
}

/**
 * Whether the option card can actually draw this call.
 *
 * `ask_user` streams its arguments, and the backend `displaySummary` fallback only
 * carries the question once arguments exist. Queueing a call before that mounts the
 * banner shell with nothing inside it — an empty bar above the composer. Both readers
 * use the same two parsers, so the banner and `AskUserOptions` always agree.
 */
export function askUserQuestionIsDrawable(toolCall: {
  arguments?: string
  displaySummary?: string
}): boolean {
  return (
    (parseAskUserArgs(toolCall.arguments) ?? parseAskUserSummary(toolCall.displaySummary)) !== null
  )
}

export function parseAskUserSelection(raw?: string): string[] {
  if (!raw?.trim()) return []
  try {
    const value = JSON.parse(raw) as { selected?: unknown }
    return Array.isArray(value.selected)
      ? value.selected.filter((item): item is string => typeof item === 'string')
      : []
  } catch {
    return []
  }
}
