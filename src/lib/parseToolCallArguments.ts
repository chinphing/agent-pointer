/**
 * Lenient tool-arguments parser for UI (aligned with backend unwrap habits).
 * Tolerates markdown fences, JSON-string wrappers, and trailing junk after a
 * complete value (common stream pollution like an extra `}`).
 */

function stripOptionalCodeFence(raw: string): string {
  const s = raw.trim()
  if (!s.startsWith('```')) return s
  const lines = s.split('\n')
  if (lines[0]?.trimStart().startsWith('`')) lines.shift()
  while (lines.length) {
    const last = lines[lines.length - 1]?.trim() ?? ''
    if (last === '```' || last === '') lines.pop()
    else break
  }
  return lines.join('\n').trim()
}

/** Index just past the first complete JSON value, or null if none. */
export function endOfFirstJsonValue(text: string): number | null {
  let i = 0
  while (i < text.length && /\s/.test(text[i]!)) i++
  if (i >= text.length) return null

  const start = text[i]!
  if (start === '"' || start === '{' || start === '[') {
    let depth = 0
    let inString = false
    let escape = false
    for (; i < text.length; i++) {
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
        if (ch === '"') {
          inString = false
          if (depth === 0 && start === '"') return i + 1
        }
        continue
      }
      if (ch === '"') {
        inString = true
        continue
      }
      if (ch === '{' || ch === '[') {
        depth++
        continue
      }
      if (ch === '}' || ch === ']') {
        depth--
        if (depth === 0) return i + 1
      }
    }
    return null
  }

  // true / false / null / number
  const rest = text.slice(i)
  const match = rest.match(/^(true|false|null|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/)
  if (!match) return null
  return i + match[0].length
}

function tryJsonParse(text: string): unknown | undefined {
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

function unwrapArgumentsLayers(value: unknown): unknown {
  let v = value
  for (let n = 0; n < 3; n++) {
    if (typeof v === 'string') {
      const next = tryJsonParse(v.trim())
      if (next === undefined) break
      v = next
      continue
    }
    break
  }
  if (v && typeof v === 'object' && !Array.isArray(v)) {
    const obj = v as Record<string, unknown>
    for (const key of ['arguments', 'params', 'parameters', 'input']) {
      const inner = obj[key]
      if (inner && typeof inner === 'object' && !Array.isArray(inner)) return inner
      if (typeof inner === 'string') {
        const nested = parseToolCallArguments(inner)
        if (nested && typeof nested === 'object' && !Array.isArray(nested)) return nested
      }
    }
  }
  return v
}

/** Parse tool call `arguments` JSON; throws if nothing usable is found. */
export function parseToolCallArguments(raw: string | undefined): unknown {
  const cleaned = stripOptionalCodeFence(raw ?? '')
  if (!cleaned) throw new Error('tool arguments are empty')

  let parsed = tryJsonParse(cleaned)
  if (parsed === undefined) {
    const end = endOfFirstJsonValue(cleaned)
    if (end != null && end < cleaned.length) {
      const head = cleaned.slice(0, end).trimEnd()
      parsed = tryJsonParse(head)
    }
  }
  if (parsed === undefined) {
    JSON.parse(cleaned) // throw the engine's error message
  }
  return unwrapArgumentsLayers(parsed)
}

export function parseToolCallArgumentsObject(
  raw: string | undefined
): Record<string, unknown> | null {
  try {
    const parsed = parseToolCallArguments(raw)
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>
    }
    return null
  } catch {
    return null
  }
}
