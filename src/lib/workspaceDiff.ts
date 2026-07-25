import type { DiffLine } from '../components/chat/DiffView.vue'

export interface ParsedWorkspaceDiff {
  lines: DiffLine[]
  stats: { adds: number; dels: number }
}

/** Convert a unified Git diff into the compact line model used by DiffView. */
export function parseWorkspaceDiff(diff: string): ParsedWorkspaceDiff {
  const lines: DiffLine[] = []
  let adds = 0
  let dels = 0

  for (const raw of diff.split('\n')) {
    if (!raw || raw.startsWith('diff --git ') || raw.startsWith('index ') || raw.startsWith('--- ') || raw.startsWith('+++ ')) {
      continue
    }
    if (raw.startsWith('@@')) {
      lines.push({ type: 'collapse', text: raw, hidden: [] })
    } else if (raw.startsWith('+')) {
      lines.push({ type: 'ins', text: raw.slice(1) })
      adds++
    } else if (raw.startsWith('-')) {
      lines.push({ type: 'del', text: raw.slice(1) })
      dels++
    } else if (raw.startsWith(' ')) {
      lines.push({ type: 'unchanged', text: raw.slice(1) })
    } else if (raw === '\\ No newline at end of file') {
      lines.push({ type: 'unchanged', text: raw })
    }
  }

  return { lines, stats: { adds, dels } }
}
