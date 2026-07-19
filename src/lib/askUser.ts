export interface AskUserOption {
  label: string
  description?: string
}

export interface AskUserArgs {
  question: string
  options: AskUserOption[]
  multiSelect: boolean
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
    if (!Array.isArray(value.options)) return null
    const options = value.options
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
