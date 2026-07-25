import type { DiffLine } from '../components/chat/DiffView.vue'

export interface DiffSearchMatch {
  key: string
  parentIndex?: number
}

export function findDiffMatches(lines: DiffLine[], query: string): DiffSearchMatch[] {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return []
  const matches: DiffSearchMatch[] = []
  lines.forEach((line, index) => {
    if (line.type === 'collapse') {
      line.hidden?.forEach((text, hiddenIndex) => {
        if (text.toLocaleLowerCase().includes(needle)) {
          matches.push({ key: `hidden-${index}-${hiddenIndex}`, parentIndex: index })
        }
      })
    } else if (line.text.toLocaleLowerCase().includes(needle)) {
      matches.push({ key: `line-${index}` })
    }
  })
  return matches
}

export function findDiffChangeBlocks(lines: DiffLine[]): string[] {
  const blocks: string[] = []
  let inChange = false
  lines.forEach((line, index) => {
    const changed = line.type === 'ins' || line.type === 'del'
    if (changed && !inChange) blocks.push(`line-${index}`)
    inChange = changed
  })
  return blocks
}

export function showDiffWhitespace(text: string): string {
  return text.replace(/\t/g, '→\t').replace(/ /g, '·')
}
