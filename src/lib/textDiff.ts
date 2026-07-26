import type { DiffLine } from '../components/chat/DiffView.vue'
import { parseToolCallArgumentsObject } from './parseToolCallArguments'

/** Match backend `text_diff::COLLAPSE_THRESHOLD`. */
const COLLAPSE_THRESHOLD = 6

export type DiffStats = { adds: number; dels: number }

function splitLines(text: string): string[] {
  if (!text) return []
  // Keep parity with similar::TextDiff::from_lines (no trailing empty from final \n alone).
  const normalized = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
  const parts = normalized.split('\n')
  if (parts.length && parts[parts.length - 1] === '') parts.pop()
  return parts
}

/**
 * Line-oriented LCS diff with collapse folding — aligns with backend `compute_diff_lines`
 * for tool-row previews (oldString/newString snippets).
 */
export function computeDiffLines(oldText: string, newText: string): {
  diffLines: DiffLine[]
  diffStats: DiffStats
} {
  const oldLines = splitLines(oldText)
  const newLines = splitLines(newText)
  const n = oldLines.length
  const m = newLines.length

  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i]![j] = oldLines[i] === newLines[j]
        ? dp[i + 1]![j + 1]! + 1
        : Math.max(dp[i + 1]![j]!, dp[i]![j + 1]!)
    }
  }

  const raw: DiffLine[] = []
  let adds = 0
  let dels = 0
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (oldLines[i] === newLines[j]) {
      raw.push({ type: 'unchanged', text: oldLines[i]! })
      i += 1
      j += 1
    } else if (dp[i + 1]![j]! >= dp[i]![j + 1]!) {
      raw.push({ type: 'del', text: oldLines[i]! })
      dels += 1
      i += 1
    } else {
      raw.push({ type: 'ins', text: newLines[j]! })
      adds += 1
      j += 1
    }
  }
  while (i < n) {
    raw.push({ type: 'del', text: oldLines[i]! })
    dels += 1
    i += 1
  }
  while (j < m) {
    raw.push({ type: 'ins', text: newLines[j]! })
    adds += 1
    j += 1
  }

  return { diffLines: foldUnchanged(raw), diffStats: { adds, dels } }
}

function foldUnchanged(all: DiffLine[]): DiffLine[] {
  const folded: DiffLine[] = []
  let index = 0
  while (index < all.length) {
    if (all[index]!.type !== 'unchanged') {
      folded.push(all[index]!)
      index += 1
      continue
    }
    const start = index
    while (index < all.length && all[index]!.type === 'unchanged') index += 1
    const count = index - start
    if (count > COLLAPSE_THRESHOLD) {
      for (let k = start; k < start + 3; k++) folded.push(all[k]!)
      const hidden: string[] = []
      for (let k = start + 3; k < index - 3; k++) hidden.push(all[k]!.text)
      folded.push({ type: 'collapse', text: String(hidden.length), hidden })
      for (let k = index - 3; k < index; k++) folded.push(all[k]!)
    } else {
      for (let k = start; k < index; k++) folded.push(all[k]!)
    }
  }
  return folded
}

/** Read oldString/newString from a file_edit tool arguments JSON. */
export function fileEditSnippetFromArgs(argumentsJson: string | undefined): {
  oldString: string
  newString: string
} | null {
  const args = parseToolCallArgumentsObject(argumentsJson)
  if (!args) return null
  const oldString = args.oldString ?? args.old_string
  const newString = args.newString ?? args.new_string
  if (typeof oldString !== 'string' || typeof newString !== 'string') return null
  return { oldString, newString }
}
