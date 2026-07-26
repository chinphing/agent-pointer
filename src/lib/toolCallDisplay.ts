import type { ToolCall } from '../types/chat'
import { taskBoardPatchSummaryFromArgs, toolCallBaseName } from './messageTooling'

function strField(args: Record<string, unknown>, keys: string[]): string {
  for (const k of keys) {
    const v = args[k]
    if (typeof v === 'string' && v.trim()) return v.trim()
  }
  return ''
}

function parseToolArgs(argumentsJson: string | undefined): Record<string, unknown> {
  if (!argumentsJson?.trim()) return {}
  try {
    const parsed = JSON.parse(argumentsJson)
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {}
  } catch {
    return {}
  }
}

function normalizeDisplayPath(path: string): string {
  return path.trim().replace(/^\\\\\?\\/, '').replace(/\\/g, '/').replace(/\/$/, '')
}

/**
 * Format a display-only path relative to the active workspace, or to the
 * user-level Pointer skills root when the path is under `~/.pointer/skills`.
 */
export function workspaceRelativeDisplayPath(path: string, workspaceRoot?: string): string {
  const normalized = normalizeDisplayPath(path)
  const workspace = normalizeDisplayPath(workspaceRoot ?? '')
  const normalizedLower = normalized.toLocaleLowerCase()

  // Keep the active workspace as the most specific display root.
  if (workspace) {
    const workspaceLower = workspace.toLocaleLowerCase()
    if (normalizedLower.startsWith(`${workspaceLower}/`)) {
      return normalized.slice(workspace.length + 1)
    }
  }

  const pointerSkillsRootMatch = normalized.match(
    /^(?:~|\/Users\/[^/]+|\/home\/[^/]+|\/root|[A-Za-z]:\/Users\/[^/]+)\/\.pointer\/skills\//i
  )
  if (pointerSkillsRootMatch) {
    return normalized.slice(pointerSkillsRootMatch[0].length)
  }
  return normalized
}

/** Extract the primary path shown beside a file-family tool call. */
export function fileToolDisplayPath(tc: ToolCall, workspaceRoot?: string): string {
  if (!isFileTool(toolCallBaseName(tc.name))) return ''
  const args = parseToolArgs(tc.arguments)
  const path = strField(args, ['path', 'file', 'base'])
  return path ? workspaceRelativeDisplayPath(path, workspaceRoot) : ''
}

/** True when backend `displayLabel` is just the raw tool slug (pre-localization). */
function isSlugToolDisplayLabel(tc: ToolCall, backendLabel: string): boolean {
  const base = toolCallBaseName(tc.name)
  return backendLabel === tc.name || backendLabel === base
}

/** Client fallback when backend display fields are missing (e.g. reloaded history). */
export function resolveToolDisplayForCall(tc: ToolCall): { label: string; summary: string } {
  const base = toolCallBaseName(tc.name)
  const args = parseToolArgs(tc.arguments)

  if (base === 'ask_user') {
    // Question is rendered once in AskUserOptions; keep the tool header label-only.
    return {
      label: '询问用户',
      summary: ''
    }
  }
  if (base === 'launch_app') {
    return {
      label: '启动应用',
      summary: truncateToolSummary(strField(args, ['app']) || strField(args, ['goal']))
    }
  }
  if (base === 'list_apps') {
    return {
      label: '列出应用',
      summary: truncateToolSummary(strField(args, ['goal']))
    }
  }
  if (base === 'cron_job') {
    const action = inferCronJobAction(args)
    return {
      label: cronJobActionLabel(action),
      summary: truncateToolSummary(cronJobSummary(action, args))
    }
  }

  return { label: tc.displayLabel?.trim() || tc.name, summary: tc.displaySummary?.trim() || '' }
}

/** Prefer localized label; ignore stale slug labels like `cron_job` from older backend rows. */
export function effectiveToolDisplayLabel(tc: ToolCall): string {
  const resolved = resolveToolDisplayForCall(tc)
  const backend = tc.displayLabel?.trim()
  if (!backend || isSlugToolDisplayLabel(tc, backend)) return resolved.label
  return backend
}

/** Prefer backend summary when present; else derive from args/tool rules. */
export function effectiveToolDisplaySummary(tc: ToolCall): string {
  const base = toolCallBaseName(tc.name)
  // Interactive card already shows the question — never mirror it in the header.
  if (base === 'ask_user') return ''
  const resolved = resolveToolDisplayForCall(tc)
  const backend = tc.displaySummary?.trim()
  if (backend) {
    if (base === 'cron_job' && looksLikeCronJobId(backend)) {
      // Older backend rows stored job_id in displaySummary for enable/disable/delete.
    } else {
      return truncateToolSummary(backend)
    }
  }
  if (resolved.summary) return truncateToolSummary(resolved.summary)
  const fromArgs = taskBoardPatchSummaryFromArgs(tc.arguments)?.trim()
  return fromArgs ? truncateToolSummary(fromArgs) : ''
}

function inferCronJobAction(args: Record<string, unknown>): string {
  const explicit = strField(args, ['action'])
  if (explicit) return explicit
  if (strField(args, ['prompt_text']) && strField(args, ['schedule'])) return 'create'
  return 'list'
}

function cronJobActionLabel(action: string): string {
  switch (action) {
    case 'create':
      return '创建定时任务'
    case 'list':
      return '列出定时任务'
    case 'enable':
      return '启用定时任务'
    case 'disable':
      return '停用定时任务'
    case 'delete':
      return '删除定时任务'
    default:
      return '定时任务'
  }
}

function cronJobSummary(action: string, args: Record<string, unknown>): string {
  switch (action) {
    case 'create':
      return (
        strField(args, ['label'])
        || strField(args, ['schedule'])
        || strField(args, ['prompt_text']).split('\n')[0]?.trim()
        || ''
      )
    case 'list':
      return ''
    default:
      return strField(args, ['label'])
  }
}

function looksLikeCronJobId(text: string): boolean {
  return /^cron-[0-9a-f]+$/i.test(text.trim())
}

export type ToolSummaryIcon = 'explore' | 'search' | 'terminal' | 'edit'

function resolveMethod(name: string, argumentsJson?: string): string {
  const i = name.indexOf(':')
  if (i !== -1) {
    const m = name.slice(i + 1).trim()
    if (m) return m
  }
  // Flat tools: derive method from the tool name suffix.
  for (const prefix of ['file_', 'skill_', 'task_board_']) {
    if (name.startsWith(prefix)) {
      const m = name.slice(prefix.length).trim()
      if (m) return m
    }
  }
  if (!argumentsJson?.trim()) return ''
  try {
    const args = JSON.parse(argumentsJson) as Record<string, unknown>
    const method = args.method
    if (typeof method === 'string' && method.trim()) return method.trim()
  } catch {
    /* ignore */
  }
  return ''
}

/** Whether the raw tool name (or base) is a file-family tool. */
function isFileTool(base: string): boolean {
  return base.startsWith('file_')
}

/** Flatten tool name to its display category for grouping. */
function groupCategory(name: string): string {
  const base = toolCallBaseName(name)
  if (isFileTool(base)) return 'file'
  if (base === 'terminal') return 'terminal'
  if (base === 'web_search') return 'web_search'
  if (base === 'web_fetch') return 'web_fetch'
  return base
}

function isInProgress(status: ToolCall['status']): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

function pathBasename(p: string): string {
  const t = p.trim()
  if (!t) return ''
  return t.split(/[/\\]/).pop() ?? t
}

function addPathBasename(raw: string, into: Set<string>): void {
  const base = pathBasename(raw)
  if (base) into.add(base)
}

function collectEditFileBasenames(argumentsJson: string, into: Set<string>): void {
  try {
    const args = JSON.parse(argumentsJson) as Record<string, unknown>
    const edits = args.edits
    if (Array.isArray(edits)) {
      for (const entry of edits) {
        if (!entry || typeof entry !== 'object') continue
        const row = entry as Record<string, unknown>
        for (const key of ['path', 'file']) {
          const v = row[key]
          if (typeof v === 'string' && v.trim()) {
            addPathBasename(v, into)
            break
          }
        }
      }
      return
    }
    if (typeof args.path === 'string' && args.path.trim()) {
      addPathBasename(args.path, into)
    }
  } catch {
    /* ignore */
  }
}

function collectReadFileBasenames(argumentsJson: string, into: Set<string>): void {
  try {
    const args = JSON.parse(argumentsJson) as Record<string, unknown>
    const paths = args.paths
    if (Array.isArray(paths)) {
      for (const item of paths) {
        if (typeof item === 'string' && item.trim()) {
          addPathBasename(item, into)
          continue
        }
        if (!item || typeof item !== 'object') continue
        const row = item as Record<string, unknown>
        for (const key of ['path', 'file']) {
          const v = row[key]
          if (typeof v === 'string' && v.trim()) {
            addPathBasename(v, into)
            break
          }
        }
      }
      return
    }
    if (typeof args.path === 'string' && args.path.trim()) addPathBasename(args.path, into)
    if (typeof args.file === 'string' && args.file.trim()) addPathBasename(args.file, into)
  } catch {
    /* ignore */
  }
}

export interface FileChangeDiff {
  toolCallId: string
  diffLines: {
    type: 'unchanged' | 'del' | 'ins' | 'collapse'
    text: string
    hidden?: string[]
  }[]
  diffStats: { adds: number; dels: number }
}

export interface FileChangeSummary {
  path: string
  fileName: string
  kind: 'edit' | 'write'
  adds: number
  dels: number
  diffs: FileChangeDiff[]
}

type RawFileChangeResult = {
  path?: unknown
  success?: unknown
  /** Legacy full/snippet diff payload (older sessions). */
  diff_lines?: unknown
  diff_stats?: unknown
  /** Current file_edit summary stats. */
  stats?: unknown
}

function parseStats(raw: unknown): { adds: number; dels: number } {
  if (!raw || typeof raw !== 'object') return { adds: 0, dels: 0 }
  const row = raw as Record<string, unknown>
  return {
    adds: typeof row.adds === 'number' ? row.adds : 0,
    dels: typeof row.dels === 'number' ? row.dels : 0
  }
}

function parseFileChangeResult(tc: ToolCall): FileChangeSummary | null {
  const base = toolCallBaseName(tc.name)
  const method = resolveMethod(tc.name, tc.arguments)
  if (!isFileTool(base) || (method !== 'edit' && method !== 'write')) return null
  if (tc.status !== 'success' || !tc.result) return null

  try {
    const result = JSON.parse(tc.result) as RawFileChangeResult
    if (result.success !== true || typeof result.path !== 'string' || !result.path.trim()) return null
    const path = result.path.trim()
    const stats = parseStats(result.stats ?? result.diff_stats)

    const diffLines = Array.isArray(result.diff_lines)
      ? result.diff_lines.filter((line): line is FileChangeDiff['diffLines'][number] => {
          if (!line || typeof line !== 'object') return false
          const row = line as Record<string, unknown>
          return (
            (row.type === 'unchanged' || row.type === 'del' || row.type === 'ins' || row.type === 'collapse')
            && typeof row.text === 'string'
            && (typeof row.hidden === 'undefined' || Array.isArray(row.hidden))
          )
        })
      : []

    return {
      path,
      fileName: pathBasename(path),
      kind: method,
      adds: stats.adds,
      dels: stats.dels,
      diffs: diffLines.length
        ? [{ toolCallId: tc.id, diffLines, diffStats: { adds: stats.adds, dels: stats.dels } }]
        : []
    }
  } catch {
    return null
  }
}

/** Aggregate successful file edits/writes in one tool run for change review UI. */
export function buildFileChangeSummaries(tools: ToolCall[]): FileChangeSummary[] {
  const byPath = new Map<string, FileChangeSummary>()
  for (const tc of tools) {
    const change = parseFileChangeResult(tc)
    if (!change) continue
    const key = change.path.replace(/\\/g, '/').toLocaleLowerCase()
    const existing = byPath.get(key)
    if (!existing) {
      byPath.set(key, change)
      continue
    }
    existing.adds += change.adds
    existing.dels += change.dels
    existing.diffs.push(...change.diffs)
    if (change.kind === 'write') existing.kind = 'write'
  }
  return [...byPath.values()]
}

export interface ToolGroupStats {
  readFiles: number
  fileSearch: number
  webSearch: number
  listed: number
  terminal: number
  created: number
  edited: number
  editing: number
}

export function buildToolGroupStats(tools: ToolCall[]): ToolGroupStats {
  const stats: ToolGroupStats = {
    readFiles: 0,
    fileSearch: 0,
    webSearch: 0,
    listed: 0,
    terminal: 0,
    created: 0,
    edited: 0,
    editing: 0
  }
  const readFileSet = new Set<string>()
  const editedFileSet = new Set<string>()
  const editingFileSet = new Set<string>()
  const createdFileSet = new Set<string>()

  for (const tc of tools) {
    const base = toolCallBaseName(tc.name)
    const method = resolveMethod(tc.name, tc.arguments)
    if (base === 'terminal') {
      stats.terminal += 1
      continue
    }
    if (base === 'web_search') {
      stats.webSearch += 1
      continue
    }
    if (isFileTool(base)) {
      if (isInProgress(tc.status) && method === 'edit') {
        collectEditFileBasenames(tc.arguments, editingFileSet)
        continue
      }
      if (isInProgress(tc.status) && method === 'write') {
        try {
          const args = JSON.parse(tc.arguments || '{}') as Record<string, unknown>
          const p = args.path ?? args.file
          if (typeof p === 'string' && p.trim()) addPathBasename(p, editingFileSet)
          else editingFileSet.add(`__write:${tc.id}`)
        } catch {
          editingFileSet.add(`__write:${tc.id}`)
        }
        continue
      }
      if (method === 'write') {
        try {
          const args = JSON.parse(tc.arguments || '{}') as Record<string, unknown>
          const p = args.path ?? args.file
          if (typeof p === 'string' && p.trim()) addPathBasename(p, createdFileSet)
          else createdFileSet.add(`__write:${tc.id}`)
        } catch {
          createdFileSet.add(`__write:${tc.id}`)
        }
        continue
      }
      if (method === 'edit') {
        collectEditFileBasenames(tc.arguments, editedFileSet)
        continue
      }
      if (method === 'glob' || method === 'grep') {
        stats.fileSearch += 1
        continue
      }
      if (method === 'list') {
        stats.listed += 1
        continue
      }
      if (method === 'read') {
        collectReadFileBasenames(tc.arguments, readFileSet)
        continue
      }
    }
    if (isInProgress(tc.status)) stats.editing += 1
  }

  stats.readFiles = readFileSet.size
  stats.edited = editedFileSet.size
  stats.created = createdFileSet.size
  stats.editing = editingFileSet.size
  return stats
}

/** Cursor-style one-line summary, e.g. 探索 14 个文件，1 次搜索 */
export function formatToolGroupSummary(stats: ToolGroupStats): string {
  const explore: string[] = []
  if (stats.readFiles) explore.push(`${stats.readFiles} 个文件`)
  const searches = stats.fileSearch + stats.webSearch
  if (searches) explore.push(`${searches} 次搜索`)
  if (stats.listed) explore.push(`${stats.listed} 次列出`)

  const parts: string[] = []
  if (explore.length) parts.push(`探索 ${explore.join('，')}`)
  if (stats.terminal) parts.push(`执行 ${stats.terminal} 条命令`)
  if (stats.created) parts.push(`创建 ${stats.created} 个文件`)
  if (stats.edited) parts.push(`编辑 ${stats.edited} 个文件`)
  if (stats.editing) parts.push(`正在编辑 ${stats.editing} 个`)
  return parts.join('，')
}

export function toolGroupSummaryIcon(stats: ToolGroupStats): ToolSummaryIcon {
  if (stats.terminal && !stats.readFiles && !stats.fileSearch && !stats.webSearch && !stats.listed) {
    return 'terminal'
  }
  if (stats.editing || stats.edited || stats.created) return 'edit'
  if (stats.fileSearch || stats.webSearch) return 'search'
  return 'explore'
}

export function toolRowMuted(tc: ToolCall): boolean {
  const base = toolCallBaseName(tc.name)
  const method = resolveMethod(tc.name, tc.arguments)
  return isFileTool(base) && method === 'read' && !isInProgress(tc.status)
}

const SHORT_LABELS: Record<string, string> = {
  '读取文件': '读取',
  '搜索内容': '搜索',
  '搜索文件': '搜索',
  '编辑文件': '编辑',
  '写入文件': '创建',
  '列出目录': '列出',
  '终端命令': '终端',
  '联网搜索': '搜索',
  '抓取网页': '抓取',
  '文件操作': '文件',
  '创建定时任务': '定时',
  '列出定时任务': '定时',
  '启用定时任务': '定时',
  '停用定时任务': '定时',
  '删除定时任务': '定时',
  '定时任务': '定时',
}

export function toolShortLabel(tc: ToolCall): string {
  const label = tc.displayLabel?.trim() || tc.name
  if (SHORT_LABELS[label]) return SHORT_LABELS[label]
  for (const [full, short] of Object.entries(SHORT_LABELS)) {
    if (label.startsWith(full)) return short
  }
  return label
}

export function toolCallDetailText(tc: ToolCall): string {
  const summary = tc.displaySummary?.trim()
  if (summary) return truncateToolSummary(summary)
  return toolShortLabel(tc)
}

const GROUPABLE_BASES = new Set(['file', 'terminal', 'web_search', 'web_fetch'])

export function isGroupableToolCall(tc: ToolCall): boolean {
  const base = toolCallBaseName(tc.name)
  return GROUPABLE_BASES.has(groupCategory(base))
}

export function canGroupToolCalls(tools: ToolCall[]): boolean {
  if (tools.length < 2) return false
  return tools.every(isGroupableToolCall)
}

export function truncateToolSummary(text: string, maxLen = 52): string {
  const t = text.trim()
  if (t.length <= maxLen) return t
  return `${t.slice(0, maxLen - 1)}…`
}

function toolInProgress(status: ToolCall['status']): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

/** One-line tool status for compact dock bar (aligns with ToolCallRow label + summary + outcome). */
export function compactToolCallStatusLine(tc: ToolCall): string {
  const label = effectiveToolDisplayLabel(tc)
  let summary = effectiveToolDisplaySummary(tc)
  if (!summary) {
    summary = taskBoardPatchSummaryFromArgs(tc.arguments)?.trim() ?? ''
  }
  if (summary) summary = truncateToolSummary(summary)

  const parts: string[] = []
  parts.push(summary ? `${label} · ${summary}` : label)

  if (toolInProgress(tc.status)) {
    parts.push('执行中')
  }

  return parts.join(' · ')
}

/** Prefer last in-progress computer tool; else last visible tool in the run. */
export function latestToolCallForCompactStatus(calls: ToolCall[]): ToolCall | undefined {
  if (!calls.length) return undefined
  for (let i = calls.length - 1; i >= 0; i -= 1) {
    if (toolInProgress(calls[i].status)) return calls[i]
  }
  return calls[calls.length - 1]
}
