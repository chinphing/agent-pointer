import type { DiffLine } from '../components/chat/DiffView.vue'

export interface ParsedWorkspaceDiff {
  lines: DiffLine[]
  stats: { adds: number; dels: number }
}

const CONTEXT_LINES = 3
const COLLAPSE_THRESHOLD = CONTEXT_LINES * 2 + 1

/** Convert a unified Git diff into the compact line model used by the right-side DiffView. */
export function parseWorkspaceDiff(diff: string): ParsedWorkspaceDiff {
  const lines: DiffLine[] = []
  let adds = 0
  let dels = 0
  let oldLine = 0
  let newLine = 0
  let unchangedRun: DiffLine[] = []

  function flushUnchanged() {
    if (!unchangedRun.length) return
    if (unchangedRun.length <= COLLAPSE_THRESHOLD) {
      lines.push(...unchangedRun)
    } else {
      lines.push(...unchangedRun.slice(0, CONTEXT_LINES))
      const hidden = unchangedRun.slice(CONTEXT_LINES, -CONTEXT_LINES)
      const first = hidden[0]!
      lines.push({
        type: 'collapse',
        text: String(hidden.length),
        hidden: hidden.map(line => line.text),
        hiddenOldLine: first.oldLine,
        hiddenNewLine: first.newLine
      })
      lines.push(...unchangedRun.slice(-CONTEXT_LINES))
    }
    unchangedRun = []
  }

  for (const raw of diff.split('\n')) {
    if (!raw || raw.startsWith('diff --git ') || raw.startsWith('index ') || raw.startsWith('--- ') || raw.startsWith('+++ ')) {
      continue
    }

    const hunk = raw.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/)
    if (hunk) {
      flushUnchanged()
      oldLine = Number(hunk[1])
      newLine = Number(hunk[3])
      lines.push({ type: 'collapse', text: raw, hidden: [], isHunkHeader: true })
      continue
    }

    if (raw.startsWith('+')) {
      flushUnchanged()
      lines.push({ type: 'ins', text: raw.slice(1), newLine })
      newLine++
      adds++
    } else if (raw.startsWith('-')) {
      flushUnchanged()
      lines.push({ type: 'del', text: raw.slice(1), oldLine })
      oldLine++
      dels++
    } else if (raw.startsWith(' ')) {
      unchangedRun.push({ type: 'unchanged', text: raw.slice(1), oldLine, newLine })
      oldLine++
      newLine++
    } else if (raw === '\\ No newline at end of file') {
      flushUnchanged()
      lines.push({ type: 'unchanged', text: raw, oldLine: oldLine - 1, newLine: newLine - 1 })
    }
  }
  flushUnchanged()

  return { lines, stats: { adds, dels } }
}