import { workspaceRichPreviewKind } from './workspacePreviewMode'

export type JsonPreviewKind = 'object' | 'array' | 'string' | 'number' | 'boolean' | 'null'

export type JsonPreviewNode = {
  id: string
  parentId: string | null
  key: string | null
  kind: JsonPreviewKind
  /** Primitive display; containers leave this empty and use children. */
  text: string
  children: JsonPreviewNode[]
}

export type ParseJsonPreviewResult =
  | { ok: true; root: JsonPreviewNode; nodeCount: number }
  | { ok: false; reason: 'invalid' | 'too_large' }

export type JsonPreviewMatch = {
  nodeId: string
  field: 'key' | 'value'
  start: number
  end: number
}

/** Skip tree preview when a file is this wide; source view still works. */
export const MAX_JSON_PREVIEW_NODES = 20_000

/** Root plus this many nested levels start expanded. */
export const JSON_PREVIEW_EXPAND_DEPTH = 2

export function isJsonPreviewPath(path: string): boolean {
  return workspaceRichPreviewKind(path) === 'json'
}

export function parseJsonPreview(
  content: string,
  maxNodes = MAX_JSON_PREVIEW_NODES
): ParseJsonPreviewResult {
  let value: unknown
  try {
    value = JSON.parse(content)
  } catch {
    return { ok: false, reason: 'invalid' }
  }
  let nodeCount = 0
  try {
    const root = buildNode('$', null, null, value, () => {
      nodeCount += 1
      if (nodeCount > maxNodes) {
        throw new Error('too_large')
      }
    })
    return { ok: true, root, nodeCount }
  } catch (err) {
    if (err instanceof Error && err.message === 'too_large') {
      return { ok: false, reason: 'too_large' }
    }
    throw err
  }
}

function buildNode(
  id: string,
  parentId: string | null,
  key: string | null,
  value: unknown,
  onNode: () => void
): JsonPreviewNode {
  onNode()
  if (value === null) {
    return { id, parentId, key, kind: 'null', text: 'null', children: [] }
  }
  if (typeof value === 'boolean') {
    return { id, parentId, key, kind: 'boolean', text: value ? 'true' : 'false', children: [] }
  }
  if (typeof value === 'number' && Number.isFinite(value)) {
    return { id, parentId, key, kind: 'number', text: String(value), children: [] }
  }
  if (typeof value === 'string') {
    return { id, parentId, key, kind: 'string', text: JSON.stringify(value), children: [] }
  }
  if (Array.isArray(value)) {
    const children = value.map((item, index) =>
      buildNode(`${id}[${index}]`, id, String(index), item, onNode)
    )
    return { id, parentId, key, kind: 'array', text: '', children }
  }
  if (value && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
    const children = entries.map(([childKey, childValue]) =>
      buildNode(`${id}.${encodeJsonKey(childKey)}`, id, childKey, childValue, onNode)
    )
    return { id, parentId, key, kind: 'object', text: '', children }
  }
  return { id, parentId, key, kind: 'string', text: JSON.stringify(String(value)), children: [] }
}

function encodeJsonKey(key: string): string {
  return encodeURIComponent(key)
}

export function isJsonContainer(node: JsonPreviewNode): boolean {
  return node.kind === 'object' || node.kind === 'array'
}

/** True when this node is an array element (id ends with `[index]`). */
export function isJsonArrayItemId(id: string): boolean {
  return /\[[0-9]+\]$/.test(id)
}

export function jsonCollapsedSummary(node: JsonPreviewNode): string {
  if (node.kind === 'object') return `{${node.children.length}}`
  if (node.kind === 'array') return `[${node.children.length}]`
  return node.text
}

export function jsonTokenKind(
  kind: JsonPreviewKind
): 'string' | 'number' | 'keyword' | 'plain' {
  if (kind === 'string') return 'string'
  if (kind === 'number') return 'number'
  if (kind === 'boolean' || kind === 'null') return 'keyword'
  return 'plain'
}

/** Collapse every container deeper than `expandDepth` (root is depth 0). */
export function defaultCollapsedIds(
  root: JsonPreviewNode,
  expandDepth = JSON_PREVIEW_EXPAND_DEPTH
): Set<string> {
  const collapsed = new Set<string>()
  walk(root, 0)
  return collapsed

  function walk(node: JsonPreviewNode, depth: number) {
    if (isJsonContainer(node) && node.children.length > 0 && depth >= expandDepth) {
      collapsed.add(node.id)
    }
    for (const child of node.children) walk(child, depth + 1)
  }
}

function indexById(root: JsonPreviewNode): Map<string, JsonPreviewNode> {
  const map = new Map<string, JsonPreviewNode>()
  const stack: JsonPreviewNode[] = [root]
  while (stack.length) {
    const node = stack.pop()!
    map.set(node.id, node)
    for (let i = node.children.length - 1; i >= 0; i--) stack.push(node.children[i]!)
  }
  return map
}

export function collectJsonAncestorIds(root: JsonPreviewNode, nodeId: string): string[] {
  const map = indexById(root)
  const ids: string[] = []
  let current = map.get(nodeId)
  while (current?.parentId) {
    ids.push(current.parentId)
    current = map.get(current.parentId)
  }
  return ids
}

export function findJsonPreviewMatches(
  root: JsonPreviewNode,
  query: string
): JsonPreviewMatch[] {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return []
  const matches: JsonPreviewMatch[] = []
  const stack: JsonPreviewNode[] = [root]
  while (stack.length) {
    const node = stack.pop()!
    if (node.key != null && !isJsonArrayItemId(node.id)) {
      pushFieldMatches(matches, node, 'key', node.key, needle)
    }
    if (!isJsonContainer(node)) {
      pushFieldMatches(matches, node, 'value', node.text, needle)
    }
    for (let i = node.children.length - 1; i >= 0; i--) stack.push(node.children[i]!)
  }
  return matches
}

function pushFieldMatches(
  matches: JsonPreviewMatch[],
  node: JsonPreviewNode,
  field: 'key' | 'value',
  haystackRaw: string,
  needle: string
) {
  if (!haystackRaw) return
  const haystack = haystackRaw.toLocaleLowerCase()
  let from = 0
  while (from <= haystack.length - needle.length) {
    const index = haystack.indexOf(needle, from)
    if (index < 0) break
    matches.push({ nodeId: node.id, field, start: index, end: index + needle.length })
    from = index + needle.length
  }
}

export type JsonPreviewSearchPart = {
  text: string
  matchIndex?: number
}

export function jsonPreviewSearchParts(
  text: string,
  nodeId: string,
  field: 'key' | 'value',
  matches: JsonPreviewMatch[]
): JsonPreviewSearchPart[] {
  const fieldMatches = matches
    .map((match, matchIndex) => ({ match, matchIndex }))
    .filter(({ match }) => match.nodeId === nodeId && match.field === field)
  if (!fieldMatches.length) return [{ text: text || '\u00A0' }]
  const parts: JsonPreviewSearchPart[] = []
  let cursor = 0
  for (const { match, matchIndex } of fieldMatches) {
    if (match.start > cursor) parts.push({ text: text.slice(cursor, match.start) })
    parts.push({ text: text.slice(match.start, match.end), matchIndex })
    cursor = match.end
  }
  if (cursor < text.length) parts.push({ text: text.slice(cursor) })
  return parts.length ? parts : [{ text: text || '\u00A0' }]
}
