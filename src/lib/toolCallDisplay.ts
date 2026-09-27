import type { ToolCall } from '../types/chat'
import { t } from '../i18n'
import { taskBoardPatchSummaryFromArgs, toolCallBaseName } from './messageTooling'
import { toolCallShowsKindLabel } from './toolCallKindIcon'

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

function stripWorkspacePrefix(normalizedPath: string, workspaceRoot: string): string | null {
  const workspace = normalizeDisplayPath(workspaceRoot)
  if (!workspace) return null
  const pathLower = normalizedPath.toLocaleLowerCase()
  const workspaceLower = workspace.toLocaleLowerCase()
  if (!pathLower.startsWith(`${workspaceLower}/`)) return null
  return normalizedPath.slice(workspace.length + 1)
}

/**
 * Format a display-only path relative to the active workspace, or to the
 * user-level Pointer skills root when the path is under `~/.pointer/skills`.
 */
export function workspaceRelativeDisplayPath(path: string, workspaceRoot?: string): string {
  const normalized = normalizeDisplayPath(path)
  const stripped = stripWorkspacePrefix(normalized, workspaceRoot ?? '')
  if (stripped != null) return stripped

  const pointerSkillsRootMatch = normalized.match(
    /^(?:~|\/Users\/[^/]+|\/home\/[^/]+|\/root|[A-Za-z]:\/Users\/[^/]+)\/\.pointer\/skills\//i
  )
  if (pointerSkillsRootMatch) {
    return normalized.slice(pointerSkillsRootMatch[0].length)
  }
  return normalized
}

/**
 * Prefer a workspace-relative path. Try each root (conversation, then project, …)
 * and keep the most specific match (shortest relative). When the lead workspace
 * was switched to a skill dir by a coder sub-agent but writes landed in the
 * session sandbox / project root, a single-root strip would keep the absolute path.
 */
export function workspaceRelativeDisplayPathWithFallbacks(
  path: string,
  roots: Array<string | null | undefined>
): string {
  const normalized = normalizeDisplayPath(path)
  let best: string | null = null
  for (const root of roots) {
    const stripped = stripWorkspacePrefix(normalized, root ?? '')
    if (stripped == null) continue
    if (best == null || stripped.length < best.length) best = stripped
  }
  if (best != null) return best

  // Last resort: path under a session sandbox but no matching root in `roots`
  // (e.g. meta still on a skill dir). Show path inside that sandbox only.
  const sandboxMatch = normalized.match(/\/session-sandboxes\/[^/]+\//i)
  if (sandboxMatch && sandboxMatch.index != null) {
    return normalized.slice(sandboxMatch.index + sandboxMatch[0].length)
  }

  return workspaceRelativeDisplayPath(path)
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
      label: t('tools.askUser'),
      summary: ''
    }
  }
  if (base === 'launch_app') {
    return {
      label: t('tools.launchApp'),
      summary: truncateToolSummary(strField(args, ['app']) || strField(args, ['goal']))
    }
  }
  if (base === 'list_apps') {
    return {
      label: t('tools.listApps'),
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
  if (base === 'job') {
    const action = strField(args, ['action']) || 'list'
    return {
      label: jobActionLabel(action),
      summary: ''
    }
  }
  if (base === 'session_search') {
    return {
      label: t('tools.sessionSearch'),
      summary: truncateToolSummary(strField(args, ['query']))
    }
  }
  if (base === 'session_read') {
    const around = strField(args, ['around_message_id'])
    const offset = args.offset
    const offsetLabel =
      typeof offset === 'number' && Number.isFinite(offset)
        ? t('tools.messageN', { n: offset })
        : typeof offset === 'string' && offset.trim()
          ? t('tools.messageN', { n: offset.trim() })
          : ''
    return {
      label: t('tools.sessionRead'),
      summary: truncateToolSummary(
        around || offsetLabel || strField(args, ['conversation_id', 'session_id'])
      )
    }
  }

  const fromArgs =
    strField(args, ['label'])
    || (base === 'media_understand' ? strField(args, ['goal', 'question']) : '')
    || (base === 'terminal'
      ? (strField(args, ['command']).split('\n')[0]?.trim() || '')
      : '')
  return {
    label: tc.displayLabel?.trim() || tc.name,
    summary: tc.displaySummary?.trim() || truncateToolSummary(fromArgs)
  }
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
      return t('tools.cronCreate')
    case 'list':
      return t('tools.cronList')
    case 'enable':
      return t('tools.cronEnable')
    case 'disable':
      return t('tools.cronDisable')
    case 'delete':
      return t('tools.cronDelete')
    default:
      return t('tools.cron')
  }
}

function jobActionLabel(action: string): string {
  switch (action) {
    case 'list':
      return t('tools.jobList')
    case 'status':
      return t('tools.jobStatus')
    case 'await':
      return t('tools.jobAwait')
    case 'cancel':
      return t('tools.jobCancel')
    default:
      return t('tools.job')
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

export function isToolCallInProgress(status: ToolCall['status']): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

export function isBackgroundSubagentCall(tc: ToolCall): boolean {
  if (toolCallBaseName(tc.name) !== 'run_subagent') return false
  if (isBackgroundJobHandleResult(tc.result)) return true
  const args = parseToolArgs(tc.arguments)
  if (args.background === true) return true
  if (args.background === false) return false
  const agentId = typeof args.agentId === 'string' ? args.agentId.trim().toLowerCase() : ''
  return agentId === 'self' || agentId === 'explore' || agentId === 'coder'
}

export function isBackgroundTerminalCall(tc: ToolCall): boolean {
  if (toolCallBaseName(tc.name) !== 'terminal') return false
  const args = parseToolArgs(tc.arguments)
  const n = args.blockUntilMs
  return typeof n === 'number' && Number.isFinite(n) && n >= 0
}

export function isBackgroundJobHost(tc: ToolCall): boolean {
  return isBackgroundSubagentCall(tc) || isBackgroundTerminalCall(tc)
}

/** Job handle JSON — not worker Markdown, terminal stdout, or `job.status` snapshots.
 *  List/status items always include `claimed`; spawn receipts do not.
 */
export function isBackgroundJobHandleResult(result?: string | null): boolean {
  if (!result?.trim()) return false
  try {
    const v = JSON.parse(result) as Record<string, unknown>
    const jobId = typeof v.jobId === 'string' ? v.jobId.trim() : ''
    const kind = typeof v.kind === 'string' ? v.kind.trim() : ''
    return (
      jobId.length > 0
      && (kind === 'subagent' || kind === 'terminal')
      && v.claimed === undefined
      && v.stdout === undefined
      && v.exitCode === undefined
      && v.content === undefined
    )
  } catch {
    return false
  }
}

/** Job id from a background host handle result, if present. */
export function backgroundJobIdFromToolCall(tc: ToolCall): string | null {
  if (!tc.result?.trim()) return null
  try {
    const v = JSON.parse(tc.result) as Record<string, unknown>
    const jobId = typeof v.jobId === 'string' ? v.jobId.trim() : ''
    return jobId.length > 0 ? jobId : null
  } catch {
    return null
  }
}

/** Handle JSON `status` field (`running` / `completed` / …), if present. */
export function backgroundHandleStatus(result?: string | null): string | null {
  if (!isBackgroundJobHandleResult(result)) return null
  try {
    const v = JSON.parse(result!) as Record<string, unknown>
    const status = typeof v.status === 'string' ? v.status.trim().toLowerCase() : ''
    return status || null
  } catch {
    return null
  }
}

/** True while the handle still claims the job is active (not terminal). */
export function isBackgroundHandleInProgress(result?: string | null): boolean {
  const status = backgroundHandleStatus(result)
  return status === 'running' || status === 'queued'
}

/** Background host row still live by tool status and/or handle JSON. */
export function isLiveBackgroundHostTool(tc: ToolCall): boolean {
  if (!isBackgroundJobHost(tc)) return false
  if (tc.status === 'failed' || tc.status === 'rejected') return false
  if (isToolCallInProgress(tc.status)) return true
  if (tc.status === 'success' && isBackgroundHandleInProgress(tc.result)) return true
  return false
}

/**
 * UI status for background hosts: trust the handle when memory `status`
 * was wrongly finalized (e.g. turn Done + handle JSON present).
 */
export function resolveBackgroundHostDisplayStatus(tc: ToolCall): ToolCall['status'] {
  if (!isBackgroundJobHost(tc)) return tc.status
  if (isLiveBackgroundHostTool(tc)) return 'running'
  const handleStatus = backgroundHandleStatus(tc.result)
  if (handleStatus === 'completed') return 'success'
  if (handleStatus === 'failed' || handleStatus === 'cancelled' || handleStatus === 'canceled') {
    return 'failed'
  }
  return tc.status
}

/** Running `job` tool with action await (parent blocked on wait). */
export function isJobAwaitCall(tc: ToolCall): boolean {
  if (toolCallBaseName(tc.name) !== 'job') return false
  const args = parseToolArgs(tc.arguments)
  const action = typeof args.action === 'string' ? args.action.trim() : ''
  return action === 'await'
}

export function toolCallProgressLabel(tc: ToolCall): string {
  if (isBackgroundJobHost(tc) && isLiveBackgroundHostTool(tc)) return t('tools.backgroundRunning')
  return t('tools.running')
}

function isInProgress(status: ToolCall['status']): boolean {
  return isToolCallInProgress(status)
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

function countContentLines(text: string): number {
  if (!text) return 0
  const normalized = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
  const parts = normalized.split('\n')
  return parts[parts.length - 1] === '' ? Math.max(0, parts.length - 1) : parts.length
}

function writeAddsFromArgs(argumentsJson: string | undefined): number {
  const args = parseToolArgs(argumentsJson ?? '')
  return typeof args.content === 'string' ? countContentLines(args.content) : 0
}

function summaryFromRetainedFileChange(tc: ToolCall): FileChangeSummary | null {
  const change = tc.fileChange
  if (!change?.path?.trim()) return null
  const path = change.path.trim()
  return {
    path,
    fileName: pathBasename(path),
    kind: change.kind,
    adds: change.adds,
    dels: change.dels,
    diffs: []
  }
}

/** Snapshot path and +/- before the body group is cleared. */
export function rememberFileChange(tc: ToolCall): void {
  if (tc.fileChange?.path?.trim()) return
  const parsed = parseFileChangeResult(tc)
  if (!parsed) return
  tc.fileChange = {
    path: parsed.path,
    kind: parsed.kind,
    adds: parsed.adds,
    dels: parsed.dels
  }
}

function parseFileChangeResult(tc: ToolCall): FileChangeSummary | null {
  if (tc.bodyEvicted) return summaryFromRetainedFileChange(tc)
  const base = toolCallBaseName(tc.name)
  const method = resolveMethod(tc.name, tc.arguments)
  if (!isFileTool(base) || (method !== 'edit' && method !== 'write')) return null
  if (tc.status !== 'success' || !tc.result) return null

  try {
    const result = JSON.parse(tc.result) as RawFileChangeResult
    if (result.success !== true || typeof result.path !== 'string' || !result.path.trim()) return null
    const path = result.path.trim()
    const hasStats = result.stats != null || result.diff_stats != null
    let stats = parseStats(result.stats ?? result.diff_stats)
    if (!hasStats && method === 'write') {
      stats = { adds: writeAddsFromArgs(tc.arguments), dels: 0 }
    }

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
  skill: number
  mediaUnderstand: number
  other: number
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
    editing: 0,
    skill: 0,
    mediaUnderstand: 0,
    other: 0
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
    if (base === 'skill_read' || base === 'skill_import') {
      stats.skill += 1
      continue
    }
    if (base === 'media_understand') {
      stats.mediaUnderstand += 1
      continue
    }
    if (base === 'session_search' || base === 'session_read' || base === 'memory' || base === 'read_lints') {
      stats.fileSearch += 1
      continue
    }
    if (isInProgress(tc.status)) stats.editing += 1
    else stats.other += 1
  }

  stats.readFiles = readFileSet.size
  stats.edited = editedFileSet.size
  stats.created = createdFileSet.size
  stats.editing = editingFileSet.size
  return stats
}

/** Cursor-style one-line summary, e.g. explored 14 files, 1 search */
export function formatToolGroupSummary(stats: ToolGroupStats): string {
  const join = t('tools.summaryJoin')
  const explore: string[] = []
  if (stats.readFiles) explore.push(t('tools.filesCount', { count: stats.readFiles }))
  const searches = stats.fileSearch + stats.webSearch
  if (searches) explore.push(t('tools.searchesCount', { count: searches }))
  if (stats.listed) explore.push(t('tools.listedCount', { count: stats.listed }))

  const parts: string[] = []
  if (explore.length) parts.push(t('tools.exploreJoined', { parts: explore.join(join) }))
  if (stats.terminal) parts.push(t('tools.ranCommands', { count: stats.terminal }))
  if (stats.created) parts.push(t('tools.createdFiles', { count: stats.created }))
  if (stats.edited) parts.push(t('tools.editedFiles', { count: stats.edited }))
  if (stats.editing) parts.push(t('tools.editingFiles', { count: stats.editing }))
  if (stats.skill) parts.push(t('tools.skillTimes', { count: stats.skill }))
  if (stats.mediaUnderstand) parts.push(t('tools.understandTimes', { count: stats.mediaUnderstand }))
  if (stats.other) parts.push(t('tools.otherOps', { count: stats.other }))
  return parts.join(join)
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

/** Full label key → short label key (resolved via current UI locale). */
const SHORT_LABEL_KEYS: Array<[string, string]> = [
  ['tools.fileRead', 'tools.shortRead'],
  ['tools.fileGrep', 'tools.shortSearch'],
  ['tools.fileGlob', 'tools.shortSearch'],
  ['tools.fileEdit', 'tools.shortEdit'],
  ['tools.fileWrite', 'tools.shortCreate'],
  ['tools.fileList', 'tools.shortList'],
  ['tools.terminal', 'tools.shortTerminal'],
  ['tools.webSearch', 'tools.shortSearch'],
  ['tools.webFetch', 'tools.shortFetch'],
  ['tools.fileOp', 'tools.shortFile'],
  ['tools.cronCreate', 'tools.shortCron'],
  ['tools.cronList', 'tools.shortCron'],
  ['tools.cronEnable', 'tools.shortCron'],
  ['tools.cronDisable', 'tools.shortCron'],
  ['tools.cronDelete', 'tools.shortCron'],
  ['tools.cron', 'tools.shortCron'],
]

/** Legacy Chinese backend labels until Rust catalog (P4) ships. */
const LEGACY_ZH_SHORT: Record<string, string> = {
  读取文件: 'tools.shortRead',
  搜索内容: 'tools.shortSearch',
  搜索文件: 'tools.shortSearch',
  编辑文件: 'tools.shortEdit',
  写入文件: 'tools.shortCreate',
  列出目录: 'tools.shortList',
  终端命令: 'tools.shortTerminal',
  联网搜索: 'tools.shortSearch',
  抓取网页: 'tools.shortFetch',
  文件操作: 'tools.shortFile',
  创建定时任务: 'tools.shortCron',
  列出定时任务: 'tools.shortCron',
  启用定时任务: 'tools.shortCron',
  停用定时任务: 'tools.shortCron',
  删除定时任务: 'tools.shortCron',
  定时任务: 'tools.shortCron',
}

export function toolShortLabel(tc: ToolCall): string {
  const label = tc.displayLabel?.trim() || tc.name
  for (const [fullKey, shortKey] of SHORT_LABEL_KEYS) {
    const full = t(fullKey)
    if (label === full || label.startsWith(full)) return t(shortKey)
  }
  if (LEGACY_ZH_SHORT[label]) return t(LEGACY_ZH_SHORT[label])
  for (const [full, shortKey] of Object.entries(LEGACY_ZH_SHORT)) {
    if (label.startsWith(full)) return t(shortKey)
  }
  return label
}

export function toolCallDetailText(tc: ToolCall): string {
  const summary = tc.displaySummary?.trim()
  if (summary) return truncateToolSummary(summary)
  return toolShortLabel(tc)
}

const UNGROUPABLE_BASES = new Set([
  'ask_user',
  'run_subagent',
  'image_generate',
  'video_generate'
])

/** Host `run_subagent` stays a normal tool row (expand args). Stats live in SubAgentFrame. */
export function shouldPinSubAgentHostRow(tc: ToolCall): boolean {
  return tc.status === 'pending_approval' || tc.waitingForInput === true
}

/** Process tools that can collapse into a Cursor-style summary row. */
export function isGroupableToolCall(tc: ToolCall): boolean {
  if (tc.status === 'pending_approval' || tc.waitingForInput === true) return false
  const base = toolCallBaseName(tc.name)
  return !UNGROUPABLE_BASES.has(base)
}

export function canGroupToolCalls(tools: ToolCall[]): boolean {
  if (tools.length < 2) return false
  return tools.every(isGroupableToolCall)
}

export type ToolCallListItem =
  | { kind: 'single'; tool: ToolCall }
  | { kind: 'group'; tools: ToolCall[]; live?: ToolCall }

function splitTrailingInProgress(tools: ToolCall[]): { done: ToolCall[]; live: ToolCall[] } {
  const live: ToolCall[] = []
  let end = tools.length
  while (end > 0 && isInProgress(tools[end - 1]!.status)) {
    end -= 1
    live.unshift(tools[end]!)
  }
  return { done: tools.slice(0, end), live }
}

function emitGroupableRun(
  run: ToolCall[],
  items: ToolCallListItem[],
  holdLiveSlot: boolean
): void {
  const { done, live } = splitTrailingInProgress(run)
  // A lone in-progress tool stays a full row — an empty summary line
  // plus a slide-in current-task looks like a blank chevron row.
  const holdGroup = holdLiveSlot && done.length >= 1
  if (done.length >= 2 || (done.length >= 1 && live.length > 0) || holdGroup) {
    items.push({ kind: 'group', tools: done, live: live[0] })
    for (const extra of live.slice(1)) items.push({ kind: 'single', tool: extra })
    return
  }
  if (done.length === 1) items.push({ kind: 'single', tool: done[0]! })
  for (const tool of live) items.push({ kind: 'single', tool })
}

/** Collapse consecutive finished process tools; keep live / interactive rows visible. */
export function partitionCollapsedToolCalls(
  tools: ToolCall[],
  opts?: { holdLiveSlot?: boolean }
): ToolCallListItem[] {
  const holdLiveSlot = opts?.holdLiveSlot === true
  const items: ToolCallListItem[] = []
  let run: ToolCall[] = []
  const flush = () => {
    if (run.length) {
      emitGroupableRun(run, items, holdLiveSlot)
      run = []
    }
  }
  for (const tool of tools) {
    if (isGroupableToolCall(tool)) {
      run.push(tool)
    } else {
      flush()
      items.push({ kind: 'single', tool })
    }
  }
  flush()
  const pinned: ToolCallListItem[] = []
  const rest: ToolCallListItem[] = []
  for (const item of items) {
    if (
      item.kind === 'single'
      && isBackgroundJobHost(item.tool)
      && isInProgress(item.tool.status)
    ) {
      pinned.push(item)
    } else {
      rest.push(item)
    }
  }
  return [...pinned, ...rest]
}

/**
 * Parent「思考中」must not stack beside an in-flight `run_subagent` /
 * background host — SubAgentFrame (or the host row) already owns the live surface.
 */
export function parentThinkingSuppressedByHost(tools: ToolCall[]): boolean {
  return tools.some(tc => {
    if (isLiveBackgroundHostTool(tc)) return true
    return (
      toolCallBaseName(tc.name) === 'run_subagent'
      && isToolCallInProgress(tc.status)
    )
  })
}

/**
 * Same as `partitionCollapsedToolCalls`, plus an empty live group so
 * first-round「思考中」occupies the collapsed-run header before any tool exists.
 *
 * Also appends a trailing empty group when the list ends on a **finished**
 * ungroupable single (e.g. completed `ask_user`): otherwise the LLM pause after
 * clarify has nowhere to attach「思考中」. Do **not** append after an in-flight
 * `run_subagent` / background host — SubAgentFrame already owns the live /
 * thinking surface; a sibling「思考中」duplicates under the current tool line.
 */
export function collapsedToolListItems(
  tools: ToolCall[],
  opts?: { holdLiveSlot?: boolean; thinkingLine?: string | null }
): ToolCallListItem[] {
  const items = partitionCollapsedToolCalls(tools, { holdLiveSlot: opts?.holdLiveSlot })
  const thinking = (opts?.thinkingLine ?? '').trim()
  if (!opts?.holdLiveSlot || !thinking) return items
  if (parentThinkingSuppressedByHost(tools)) return items
  if (items.length === 0) {
    return [{ kind: 'group', tools: [] }]
  }
  const last = items[items.length - 1]
  if (last?.kind === 'single') {
    if (isToolCallInProgress(last.tool.status)) return items
    return [...items, { kind: 'group', tools: [] }]
  }
  return items
}

/** Keep the live run header mounted across thinking → first tool. */
export function collapsedLiveRunItemKey(
  item: ToolCallListItem,
  index: number,
  itemCount: number,
  runActive: boolean
): string {
  if (item.kind === 'group') {
    if (runActive && index === itemCount - 1) return 'group:live-run'
    return `group:${item.tools[0]?.id ?? item.live?.id ?? 'run'}`
  }
  return item.tool.id
}

/** Collapsed group line: Cursor-style activity counts (never a single live tool title). */
export function formatCollapsedToolGroupLine(
  tools: ToolCall[],
  _workspaceRoot?: string
): string {
  if (tools.length === 0) return ''
  const summary = formatToolGroupSummary(buildToolGroupStats(tools))
  return summary || t('tools.toolsCount', { count: tools.length })
}

/** Prefer the partitioned live tool; else any in-progress tool (parallel finish order). */
export function resolveCollapsedGroupLiveTool(
  tools: ToolCall[],
  liveTool?: ToolCall | null
): ToolCall | null {
  if (liveTool) return liveTool
  for (let i = tools.length - 1; i >= 0; i -= 1) {
    const tc = tools[i]!
    if (isInProgress(resolveBackgroundHostDisplayStatus(tc))) return tc
  }
  return null
}

/**
 * Tool-row duration: whole seconds only, hide under 1s (`1s`, `2s`).
 */
export function formatToolDurationLabel(durationMs: number | undefined): string {
  if (durationMs == null || !Number.isFinite(durationMs) || durationMs < 1000) return ''
  return `${Math.floor(durationMs / 1000)}s`
}

export function truncateToolSummary(text: string, maxLen = 52): string {
  const t = text.trim()
  if (t.length <= maxLen) return t
  return `${t.slice(0, maxLen - 1)}…`
}

/** Keep the filename when a path is too long for a one-line status string. */
export function truncatePathKeepEnd(text: string, maxLen = 52): string {
  const t = text.trim()
  if (t.length <= maxLen) return t
  return `…${t.slice(-(maxLen - 1))}`
}

function toolInProgress(status: ToolCall['status']): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

function compactToolCallSummary(
  tc: ToolCall,
  workspaceRoot?: string
): { label: string; summary: string } {
  const label = effectiveToolDisplayLabel(tc)
  const filePath = fileToolDisplayPath(tc, workspaceRoot)
  let summary = filePath
  if (!summary) summary = effectiveToolDisplaySummary(tc)
  if (!summary) {
    summary = taskBoardPatchSummaryFromArgs(tc.arguments)?.trim() ?? ''
  }
  if (summary) {
    summary = filePath ? truncatePathKeepEnd(summary) : truncateToolSummary(summary)
  }
  return { label, summary }
}

/**
 * Visible text beside the kind icon (no kind name when the icon replaces it).
 * 委派 / 询问 / unknown still include the label.
 */
export function compactToolCallLiveText(
  tc: ToolCall,
  workspaceRoot?: string
): string {
  const { label, summary } = compactToolCallSummary(tc, workspaceRoot)
  if (toolCallShowsKindLabel(tc.name)) {
    return summary ? `${label} · ${summary}` : label
  }
  return summary || label
}

/**
 * One-line tool copy: label · summary.
 * Compact dock keeps `includeStatus` (aligns with ToolCallRow).
 * Screen readers / aria use this even when the row hides the kind name.
 */
export function compactToolCallStatusLine(
  tc: ToolCall,
  workspaceRoot?: string,
  opts?: { includeStatus?: boolean }
): string {
  const { label, summary } = compactToolCallSummary(tc, workspaceRoot)
  const parts: string[] = []
  parts.push(summary ? `${label} · ${summary}` : label)

  const displayStatus = resolveBackgroundHostDisplayStatus(tc)
  if (opts?.includeStatus !== false && toolInProgress(displayStatus)) {
    parts.push(toolCallProgressLabel(tc))
  }

  return parts.join(' · ')
}

/** Prefer last in-progress computer tool; else last visible tool in the run. */
export function latestToolCallForCompactStatus(calls: ToolCall[]): ToolCall | undefined {
  if (!calls.length) return undefined
  for (let i = calls.length - 1; i >= 0; i -= 1) {
    if (toolInProgress(resolveBackgroundHostDisplayStatus(calls[i]!))) return calls[i]
  }
  return calls[calls.length - 1]
}
