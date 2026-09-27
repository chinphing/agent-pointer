import type { AppendedMessageRow } from './api'
import type { ImportReport, PluginView, DiscoveredPlugin, ExternalPluginsProbeResult } from '../types/plugin'
import type { GlobalMcpView, McpServerDecl } from '../types/mcp'
import type {
  AgentDef,
  AgentMode,
  ChatMessage,
  ComputerAnnotatedPreview,
  ChatMediaPreview,
  ComputerMonitor,
  Conversation,
  ConversationCursor,
  ConversationMeta,
  ConversationMetaPage,
  PerformanceMode,
  Project,
  ProjectCreationResult,
  ProjectCursor,
  ProjectPage,
  ConversationSearchHit,
  DebugSessionSettings,
  EffectiveSettingsView,
  MediaDepsStatus,
  PlatformSettings,
  SkillDef,
  SkillImportResult,
  ExternalSkillsProbeResult,
  StreamEvent,
  ToolDef,
  UserSettings
} from '../types/chat'
import type {
  CronJob,
  CreateCronJobInput,
  CronDeliveryTarget,
  UpdateCronJobInput,
  WebhookConfig
} from '../types/automation'

import { WEB_API_BASE } from './runtime'
import { summarizeErrorResponse } from './httpError'

/** Windows 上连接未监听端口时，fetch 可能长时间挂起；超时后尽快失败以便界面可用。 */
const REQUEST_TIMEOUT_MS = 12_000
/** 大附件 base64 JSON 上传 / 含附件的发消息体，避免误用短超时。 */
const UPLOAD_TIMEOUT_MS = 120_000

type WebRequestInit = RequestInit & {
  /** Override default 12s abort; use for large body uploads. */
  timeoutMs?: number
  /** Shown on AbortError instead of the generic “server not running” hint. */
  timeoutMessage?: string
}

function defaultTimeoutMessage(timeoutMs: number): string {
  return `请求超时（>${timeoutMs / 1000}s）：${WEB_API_BASE || '同源 /api'} 无响应。请确认已启动 pointer-server（默认 127.0.0.1:8787）；跨源时需设置 VITE_WEB_API_BASE 与 POINTER_SERVER_CORS_ORIGINS。`
}

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentSkillOverrides?: Record<string, string[]>
  agentMode?: AgentMode
  toolRoundsUsed?: number
  toolRoundsUsedSupervisor?: number
  workspaceRoot?: string
  workspaceInheritDisabled?: boolean
  performanceMode?: PerformanceMode
}

async function request<T>(path: string, init?: WebRequestInit): Promise<T> {
  const timeoutMs = init?.timeoutMs ?? REQUEST_TIMEOUT_MS
  const timeoutMessage = init?.timeoutMessage ?? defaultTimeoutMessage(timeoutMs)
  const { timeoutMs: _t, timeoutMessage: _m, ...fetchInit } = init ?? {}
  const headers = new Headers(fetchInit.headers)
  if (
    fetchInit.body &&
    !(fetchInit.body instanceof FormData) &&
    !(fetchInit.body instanceof Blob) &&
    !(fetchInit.body instanceof ArrayBuffer)
  ) {
    headers.set('Content-Type', 'application/json')
  }

  const controller = new AbortController()
  const timeoutId = window.setTimeout(() => controller.abort(), timeoutMs)
  let res: Response
  try {
    res = await fetch(`${WEB_API_BASE}${path}`, {
      ...fetchInit,
      headers,
      credentials: 'include',
      signal: fetchInit.signal ?? controller.signal
    })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      throw new Error(timeoutMessage)
    }
    throw e
  } finally {
    window.clearTimeout(timeoutId)
  }

  if (!res.ok) throw new Error(await summarizeErrorResponse(res))
  if (res.status === 204 || res.status === 202) return undefined as T
  return await res.json()
}

async function requestBlob(path: string, init?: WebRequestInit): Promise<Blob> {
  const timeoutMs = init?.timeoutMs ?? REQUEST_TIMEOUT_MS
  const timeoutMessage = init?.timeoutMessage ?? defaultTimeoutMessage(timeoutMs)
  const { timeoutMs: _t, timeoutMessage: _m, ...fetchInit } = init ?? {}
  const controller = new AbortController()
  const timeoutId = window.setTimeout(() => controller.abort(), timeoutMs)
  let res: Response
  try {
    res = await fetch(`${WEB_API_BASE}${path}`, {
      ...fetchInit,
      credentials: 'include',
      signal: fetchInit.signal ?? controller.signal
    })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      throw new Error(timeoutMessage)
    }
    throw e
  } finally {
    window.clearTimeout(timeoutId)
  }

  if (!res.ok) throw new Error(await summarizeErrorResponse(res))
  return await res.blob()
}

/**
 * Start a browser-native download immediately (progress UI + streaming).
 * Prefer this over fetch→blob for large files: cookies are sent on same-site
 * navigation, and the download bar appears before the body finishes.
 */
function triggerNativeDownload(url: string, fileName?: string) {
  const anchor = document.createElement('a')
  anchor.href = url
  const name = fileName?.trim()
  if (name) anchor.download = name
  anchor.rel = 'noopener'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
}

export async function sendChat(payload: SendChatPayload): Promise<void> {
  await request('/api/chat', {
    method: 'POST',
    body: JSON.stringify(payload),
    // History may still carry large attachment payloads on the wire.
    timeoutMs: UPLOAD_TIMEOUT_MS,
    timeoutMessage: `发送超时（>${UPLOAD_TIMEOUT_MS / 1000}s）。若含大附件请稍后重试或压缩图片；确认 pointer-server 正常后再试。`
  })
}

export async function cancelChat(
  conversationId: string,
  options?: { cancelBackgroundJobs?: boolean }
): Promise<void> {
  await request(`/api/chat/${encodeURIComponent(conversationId)}/cancel`, {
    method: 'POST',
    body: JSON.stringify({
      cancelBackgroundJobs: options?.cancelBackgroundJobs ?? true,
    }),
    timeoutMs: 20_000,
    timeoutMessage: '停止超时。请稍后重试。',
  })
}

export async function cancelBackgroundJobs(
  conversationId: string,
  jobIds?: string[]
): Promise<string[]> {
  const res = await request<{ cancelled: string[] }>(
    `/api/chat/${encodeURIComponent(conversationId)}/cancel-jobs`,
    {
      method: 'POST',
      body: JSON.stringify({ jobIds: jobIds ?? [] }),
    }
  )
  return res.cancelled ?? []
}

function workspaceQuery(
  workspaceRoot: string,
  relativePath?: string,
  extras?: Record<string, string | undefined>
): string {
  const params = new URLSearchParams({ workspaceRoot })
  if (relativePath !== undefined) params.set('relativePath', relativePath)
  if (extras) {
    for (const [key, value] of Object.entries(extras)) {
      if (value !== undefined && value !== '') params.set(key, value)
    }
  }
  return params.toString()
}

export async function listWorkspaceDirectory(
  workspaceRoot: string,
  relativePath?: string
): Promise<import('./api').WorkspaceEntry[]> {
  return await request(`/api/workspace/directory?${workspaceQuery(workspaceRoot, relativePath)}`)
}

export async function searchWorkspaceEntries(
  workspaceRoot: string,
  query: string,
  limit?: number
): Promise<import('./api').WorkspaceEntry[]> {
  const params = new URLSearchParams()
  params.set('workspace_root', workspaceRoot)
  params.set('query', query)
  if (limit != null) params.set('limit', String(limit))
  return await request(`/api/workspace/search?${params.toString()}`)
}

export async function readWorkspaceFile(
  workspaceRoot: string,
  relativePath: string
): Promise<import('./api').WorkspaceFilePreview> {
  return await request(`/api/workspace/file?${workspaceQuery(workspaceRoot, relativePath)}`)
}

/**
 * Inline image/PDF preview for workspace files (object URL; revoke when the preview unmounts).
 * `/api/workspace/file` only returns JSON metadata for binaries — use this for raw bytes.
 */
export async function workspaceFileMediaObjectUrl(
  workspaceRoot: string,
  relativePath: string
): Promise<string> {
  const blob = await requestBlob(
    `/api/workspace/file-media?${workspaceQuery(workspaceRoot, relativePath)}`,
    {
      timeoutMs: UPLOAD_TIMEOUT_MS,
      timeoutMessage: `加载工作区预览超时（>${UPLOAD_TIMEOUT_MS / 1000}s）`
    }
  )
  return URL.createObjectURL(blob)
}

export async function deleteWorkspacePath(
  workspaceRoot: string,
  relativePath: string
): Promise<void> {
  await request(`/api/workspace/path?${workspaceQuery(workspaceRoot, relativePath)}`, {
    method: 'DELETE'
  })
}

export async function getWorkspaceGitStatus(workspaceRoot: string): Promise<import('./api').GitStatusResponse> {
  return await request(`/api/workspace/git/status?${workspaceQuery(workspaceRoot)}`)
}

export async function getWorkspaceGitDiff(
  workspaceRoot: string,
  relativePath: string,
  status?: string
): Promise<import('./api').GitDiff> {
  return await request(
    `/api/workspace/git/diff?${workspaceQuery(workspaceRoot, relativePath, { status })}`
  )
}

export async function getTurnFileDiff(
  conversationId: string,
  turnId: string,
  workspaceRoot: string,
  path: string
): Promise<import('./api').TurnFileDiff> {
  const params = new URLSearchParams({
    conversationId,
    turnId,
    workspaceRoot,
    path
  })
  return await request(`/api/workspace/turn-file-diff?${params}`)
}

export async function listTurnFileChanges(
  conversationId: string,
  turnIds: string[]
): Promise<import('./api').TurnFileChangesForTurn[]> {
  const params = new URLSearchParams({
    conversationId,
    turnIds: turnIds.join(',')
  })
  return await request(`/api/workspace/turn-file-changes?${params}`)
}

export async function saveTurnFileChanges(
  conversationId: string,
  turnId: string,
  files: import('./api').TurnFileChangeEntry[]
): Promise<void> {
  await request('/api/workspace/turn-file-changes', {
    method: 'POST',
    body: JSON.stringify({ conversationId, turnId, files })
  })
}

export async function createConsoleSession(
  input: import('./api').ConsoleSessionCreateInput
): Promise<import('./api').ConsoleSessionInfo> {
  return await request('/api/console/sessions', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export async function writeConsoleSession(sessionId: string, data: string): Promise<void> {
  await request(`/api/console/sessions/${encodeURIComponent(sessionId)}/input`, {
    method: 'POST',
    body: JSON.stringify({ data })
  })
}

export async function resizeConsoleSession(sessionId: string, cols: number, rows: number): Promise<void> {
  await request(`/api/console/sessions/${encodeURIComponent(sessionId)}/resize`, {
    method: 'POST',
    body: JSON.stringify({ cols, rows })
  })
}

export async function closeConsoleSession(sessionId: string): Promise<boolean> {
  const result = await request<{ closed: boolean }>(`/api/console/sessions/${encodeURIComponent(sessionId)}`, {
    method: 'DELETE'
  })
  return result.closed
}

export async function abortTerminalCommand(
  conversationId: string,
  toolCallId?: string
): Promise<boolean> {
  const j = await request<{ aborted: boolean }>(
    `/api/chat/${encodeURIComponent(conversationId)}/abort-terminal`,
    {
      method: 'POST',
      body: JSON.stringify({ toolCallId: toolCallId ?? null })
    }
  )
  return Boolean(j?.aborted)
}

export async function approveToolCall(
  _conversationId: string,
  toolCallId: string,
  approved: boolean
): Promise<void> {
  await request(`/api/tools/${encodeURIComponent(toolCallId)}/approve`, {
    method: 'POST',
    body: JSON.stringify({ approved })
  })
}

export async function submitAskUser(toolCallId: string, selected: string[]): Promise<void> {
  await request(`/api/tools/${encodeURIComponent(toolCallId)}/ask-user`, {
    method: 'POST',
    body: JSON.stringify({ selected })
  })
}

export async function submitTerminalInput(requestId: string, text: string): Promise<void> {
  await request(`/api/terminal-input/${encodeURIComponent(requestId)}/submit`, {
    method: 'POST',
    body: JSON.stringify({ text })
  })
}

export async function dismissTerminalInput(requestId: string): Promise<void> {
  await request(`/api/terminal-input/${encodeURIComponent(requestId)}/dismiss`, {
    method: 'POST'
  })
}

export async function getSettings(): Promise<EffectiveSettingsView> {
  return await request<EffectiveSettingsView>('/api/settings')
}

export async function updateDebugSessionSettings(
  settings: DebugSessionSettings
): Promise<DebugSessionSettings> {
  return await request<DebugSessionSettings>('/api/debug-session-settings', {
    method: 'PUT',
    body: JSON.stringify(settings)
  })
}

export async function updateUserSettings(user: UserSettings): Promise<EffectiveSettingsView> {
  return await request<EffectiveSettingsView>('/api/user-settings', { method: 'PUT', body: JSON.stringify(user) })
}

export async function updatePlatformSettings(_platform: PlatformSettings): Promise<EffectiveSettingsView> {
  throw new Error('web runtime: platform settings are read-only')
}

export async function setApiKey(key: string): Promise<void> {
  await request('/api/key', { method: 'POST', body: JSON.stringify({ api_key: key }) })
}

export async function clearApiKey(): Promise<void> {
  await request('/api/key', { method: 'DELETE' })
}

export async function testConnection(): Promise<{ ok: boolean; latencyMs: number; message: string }> {
  try {
    const ms = await request<number>('/api/test-connection', { method: 'POST' })
    return { ok: true, latencyMs: Number(ms), message: '连接成功' }
  } catch (e: any) {
    return { ok: false, latencyMs: 0, message: String(e?.message || e) }
  }
}

export async function listSkills(): Promise<SkillDef[]> {
  return await request<SkillDef[]>('/api/skills')
}

export async function reloadSkillMeta(): Promise<SkillDef[]> {
  return await request<SkillDef[]>('/api/skills/reload-meta', { method: 'POST' })
}

export async function importSkillZip(file: File): Promise<SkillImportResult> {
  return await request<SkillImportResult>('/api/skills', {
    method: 'POST',
    body: await file.arrayBuffer()
  })
}

export async function probeExternalSkills(): Promise<ExternalSkillsProbeResult> {
  return await request<ExternalSkillsProbeResult>('/api/skills/external-probe')
}

export async function importExternalSkills(sourceIds: string[]): Promise<SkillImportResult> {
  return await request<SkillImportResult>('/api/skills/import-external', {
    method: 'POST',
    body: JSON.stringify({ sourceIds })
  })
}

export async function dismissExternalSkillsPrompt(): Promise<void> {
  await request('/api/skills/external-probe/dismiss', { method: 'POST' })
}

export async function listPlugins(): Promise<PluginView[]> {
  return await request<PluginView[]>('/api/plugins')
}

export async function enablePlugin(pluginId: string): Promise<PluginView> {
  return await request<PluginView>(`/api/plugins/${encodeURIComponent(pluginId)}/enable`, {
    method: 'POST'
  })
}

export async function disablePlugin(pluginId: string): Promise<PluginView> {
  return await request<PluginView>(`/api/plugins/${encodeURIComponent(pluginId)}/disable`, {
    method: 'POST'
  })
}

export async function uninstallPlugin(pluginId: string): Promise<void> {
  await request(`/api/plugins/${encodeURIComponent(pluginId)}/uninstall`, { method: 'POST' })
}

export async function importPlugin(source: string): Promise<ImportReport[]> {
  return await request<ImportReport[]>('/api/plugins', {
    method: 'POST',
    body: JSON.stringify({ source })
  })
}

export async function importPluginZip(file: File): Promise<ImportReport[]> {
  const data = await file.arrayBuffer()
  return await request<ImportReport[]>('/api/plugins/import-zip', {
    method: 'POST',
    body: new Uint8Array(data),
    headers: { 'Content-Type': 'application/zip' }
  })
}

export async function discoverPlugins(dir: string): Promise<DiscoveredPlugin[]> {
  return await request<DiscoveredPlugin[]>('/api/plugins/discover', {
    method: 'POST',
    body: JSON.stringify({ dir })
  })
}

export async function probeExternalPlugins(): Promise<ExternalPluginsProbeResult> {
  return await request<ExternalPluginsProbeResult>('/api/plugins/external-probe')
}

export async function importExternalPlugin(sourceId: string): Promise<ImportReport> {
  return await request<ImportReport>('/api/plugins/import-external', {
    method: 'POST',
    body: JSON.stringify({ sourceId })
  })
}

export async function listMcpServers(): Promise<GlobalMcpView> {
  return await request<GlobalMcpView>('/api/mcp')
}

export async function saveMcpServers(servers: McpServerDecl[]): Promise<GlobalMcpView> {
  return await request<GlobalMcpView>('/api/mcp', {
    method: 'PUT',
    body: JSON.stringify(servers)
  })
}

export async function reloadMcpServers(): Promise<GlobalMcpView> {
  return await request<GlobalMcpView>('/api/mcp/reload', { method: 'POST' })
}

export async function restartMcpServer(): Promise<GlobalMcpView> {
  return await request<GlobalMcpView>('/api/mcp/restart', { method: 'POST' })
}

export async function listTools(): Promise<ToolDef[]> {

  return await request<ToolDef[]>('/api/tools')
}

export async function listAgents(): Promise<AgentDef[]> {
  return await request<AgentDef[]>('/api/agents')
}

export async function getTaskBoardSnapshot(
  conversationId: string,
  taskId?: string
): Promise<import('../types/chat').TaskBoardDocument> {
  const q = new URLSearchParams({ conversationId })
  if (taskId?.trim()) q.set('taskId', taskId.trim())
  return await request(`/api/task-board/snapshot?${q}`)
}

export async function previewComputerAnnotatedScreen(conversationId: string): Promise<ComputerAnnotatedPreview> {
  return await request<ComputerAnnotatedPreview>(`/api/computer/annotated-preview?conversationId=${encodeURIComponent(conversationId)}`)
}

export async function previewComputerRoundScreen(relPath: string): Promise<ComputerAnnotatedPreview> {
  const q = new URLSearchParams({ relPath })
  return await request<ComputerAnnotatedPreview>(`/api/computer/round-screen-preview?${q}`)
}

export async function previewChatMedia(storageRelPath: string): Promise<ChatMediaPreview> {
  const q = new URLSearchParams({ storageRelPath })
  return await request<ChatMediaPreview>(`/api/chat/media-preview?${q}`)
}

export async function previewMediaRef(mediaRef: string): Promise<ChatMediaPreview> {
  const q = new URLSearchParams({ mediaRef })
  return await request<ChatMediaPreview>(`/api/chat/media-ref-preview?${q}`)
}

export async function saveChatAttachment(
  payload: import('./api').SaveChatAttachmentPayload,
  onProgress?: (p: import('./api').AttachmentUploadProgress) => void,
  options?: { signal?: AbortSignal }
): Promise<string> {
  if (!payload.file) {
    throw new Error('web saveChatAttachment requires File (multipart)')
  }
  const form = new FormData()
  form.append('conversationId', payload.conversationId)
  form.append('attachmentId', payload.attachmentId)
  form.append('fileName', payload.fileName || payload.file.name)
  form.append('file', payload.file, payload.fileName || payload.file.name)

  const { postMultipartJson } = await import('./multipartUpload')
  const res = await postMultipartJson<{ storageRelPath: string }>(
    '/api/chat/save-attachment',
    form,
    {
      timeoutMs: UPLOAD_TIMEOUT_MS,
      onProgress,
      signal: options?.signal
    }
  )
  return res.storageRelPath
}

export async function checkMediaDeps(): Promise<MediaDepsStatus> {
  return await request<MediaDepsStatus>('/api/media/deps')
}

export async function revealInFinder(_path: string): Promise<void> {
  // Web 端不支持在 Finder 中显示
  throw new Error('revealInFinder is not supported in web runtime')
}

export async function openPathWithDefaultApp(_path: string): Promise<void> {
  throw new Error('openPathWithDefaultApp is not supported in web runtime')
}

export async function openChatMedia(storageRelPath: string, fileName?: string): Promise<void> {
  await downloadChatMedia(storageRelPath, fileName)
}

/**
 * Authenticated chat media download via browser navigation (session cookie).
 * Do not use fetch→blob here: large files would only show a download after the
 * entire body was buffered (and the 12s request timeout would abort them).
 */
export async function downloadChatMedia(
  storageRelPath: string,
  fileName?: string
): Promise<void> {
  const params = new URLSearchParams({ storageRelPath: storageRelPath.trim() })
  const name = fileName?.trim()
  if (name) params.set('fileName', name)
  triggerNativeDownload(
    `${WEB_API_BASE}/api/chat/media-download?${params}`,
    name || 'attachment'
  )
}

export async function downloadChatMediaRef(mediaRef: string, fileName?: string): Promise<void> {
  const params = new URLSearchParams({ mediaRef: mediaRef.trim() })
  const name = fileName?.trim()
  if (name) params.set('fileName', name)
  triggerNativeDownload(
    `${WEB_API_BASE}/api/chat/media-ref-download?${params}`,
    name || 'attachment'
  )
}

/** Inline video preview URL with auth (object URL; revoke when the element unmounts). */
export async function chatMediaStreamObjectUrl(storageRelPath: string): Promise<string> {
  const q = new URLSearchParams({ storageRelPath: storageRelPath.trim() })
  const blob = await requestBlob(`/api/chat/media-stream?${q}`)
  return URL.createObjectURL(blob)
}

export function chatMediaDownloadUrl(storageRelPath: string, fileName?: string): string {
  const params = new URLSearchParams({ storageRelPath: storageRelPath.trim() })
  const name = fileName?.trim()
  if (name) params.set('fileName', name)
  return `${WEB_API_BASE}/api/chat/media-download?${params}`
}

export function chatMediaStreamUrl(storageRelPath: string): string {
  const q = encodeURIComponent(storageRelPath.trim())
  return `${WEB_API_BASE}/api/chat/media-stream?storageRelPath=${q}`
}

export function chatMediaRefDownloadUrl(mediaRef: string, fileName?: string): string {
  const params = new URLSearchParams({ mediaRef: mediaRef.trim() })
  const name = fileName?.trim()
  if (name) params.set('fileName', name)
  return `${WEB_API_BASE}/api/chat/media-ref-download?${params}`
}

export async function captureManualDesktopSnapshot(): Promise<Blob> {
  return await requestBlob('/api/computer/manual-snapshot', { method: 'POST' })
}

export async function readLocalFileForAttachment(_path: string): Promise<import('./api').LocalFileAttachmentPayload> {
  throw new Error('readLocalFileForAttachment is not supported in web runtime')
}

export async function listComputerMonitors(): Promise<ComputerMonitor[]> {
  return await request<ComputerMonitor[]>('/api/computer/monitors')
}

export async function setComputerCompactChrome(_compact: boolean): Promise<void> {
  /* Web: no OS window chrome */
}

export async function beginComputerCompactWindow(): Promise<void> {
  /* Web: no OS window chrome */
}

export async function restoreComputerCompactWindowNative(): Promise<void> {
  /* Web: no OS window chrome */
}

export async function placeComputerCompactWindow(
  _width: number,
  _height: number,
  _margin: number
): Promise<void> {
  /* Web: no OS window placement */
}

export async function reapplyWindowChrome(): Promise<void> {
  /* Web: no OS window chrome */
}

export async function setComputerConversationMonitor(conversationId: string, monitorId: string | null): Promise<void> {
  await request('/api/computer/monitor', {
    method: 'POST',
    body: JSON.stringify({ conversationId, monitorId })
  })
}

export async function confirmComputerMonitorPick(conversationId: string): Promise<void> {
  await request(`/api/computer/monitor-pick/${encodeURIComponent(conversationId)}/confirm`, {
    method: 'POST'
  })
}

export async function cancelComputerMonitorPick(conversationId: string): Promise<void> {
  await request(`/api/computer/monitor-pick/${encodeURIComponent(conversationId)}/cancel`, {
    method: 'POST'
  })
}

export async function loadConversationMessage(
  conversationId: string,
  messageId: string
): Promise<ChatMessage | null> {
  const id = messageId.trim()
  if (!conversationId.trim() || !id) {
    console.warn('[web] loadConversationMessage skipped: empty id', conversationId, messageId)
    return null
  }
  return await request<ChatMessage | null>(
    `/api/conversations/${encodeURIComponent(conversationId)}/messages/${encodeURIComponent(id)}`
  )
}

export async function loadConversationMessages(
  conversationId: string
): Promise<ChatMessage[]> {
  // Use the dedicated per-conversation route; the legacy pattern of
  // re-fetching all conversations and filtering client-side was a major waste
  // on web (it pulled every message of every conversation on every hydrate).
  return await request<ChatMessage[]>(
    `/api/conversations/${encodeURIComponent(conversationId)}/messages`
  )
}

export async function loadConversationMessagesPage(
  conversationId: string,
  opts?: import('./api').LoadConversationMessagesPageOpts
): Promise<import('./api').ConversationMessagePage> {
  const params = new URLSearchParams()
  if (opts?.limitTurns != null) params.set('limitTurns', String(opts.limitTurns))
  if (opts?.beforePosition != null) params.set('beforePosition', String(opts.beforePosition))
  if (opts?.afterPosition != null) params.set('afterPosition', String(opts.afterPosition))
  if (opts?.aroundMessageId?.trim()) params.set('aroundMessageId', opts.aroundMessageId.trim())
  if (opts?.includeScopedSubMessages === true) params.set('includeScopedSubMessages', 'true')
  // Always send at least limitTurns so the server returns MessagePage (not legacy Vec).
  if (
    !params.has('limitTurns')
    && !params.has('beforePosition')
    && !params.has('afterPosition')
    && !params.has('aroundMessageId')
  ) {
    params.set('limitTurns', '8')
  }
  const q = params.toString()
  return await request(
    `/api/conversations/${encodeURIComponent(conversationId)}/messages?${q}`
  )
}

export async function loadScopedSubMessagesForTrace(
  conversationId: string,
  opts: import('./api').LoadScopedSubMessagesOpts
): Promise<import('../types/chat').ChatMessage[]> {
  const params = new URLSearchParams()
  params.set('anchorMessageId', opts.anchorMessageId.trim())
  if (opts.traceId?.trim()) params.set('traceId', opts.traceId.trim())
  if (opts.agentInstanceId?.trim()) params.set('agentInstanceId', opts.agentInstanceId.trim())
  return await request(
    `/api/conversations/${encodeURIComponent(conversationId)}/scoped-messages?${params}`
  )
}

export async function loadConversations(): Promise<Conversation[]> {
  return await request<Conversation[]>('/api/conversations')
}

/** Cursor-paginated meta-only list (no messages). Sort: pinned DESC, updatedAt DESC, id DESC. */
export async function loadConversationMetas(
  cursor: ConversationCursor | null,
  limit = 50
): Promise<ConversationMetaPage> {
  const params = new URLSearchParams()
  params.set('limit', String(limit))
  if (cursor) {
    params.set('cursor_updated_at', String(cursor.updatedAt))
    params.set('cursor_id', cursor.id)
  }
  const items = await request<ConversationMeta[]>(`/api/conversations/meta?${params.toString()}`)
  const nextCursor =
    items.length === limit && items.length > 0
      ? { updatedAt: items[items.length - 1]!.updatedAt, id: items[items.length - 1]!.id }
      : null
  return { items, nextCursor }
}

/** Single conversation meta by id (null when missing or out of list scope). */
export async function loadConversationMeta(
  conversationId: string
): Promise<ConversationMeta | null> {
  return await request<ConversationMeta | null>(
    `/api/conversations/${encodeURIComponent(conversationId)}/meta`
  )
}

export async function loadProjects(cursor: ProjectCursor | null, limit = 20): Promise<ProjectPage> {
  const params = new URLSearchParams({ limit: String(limit) })
  if (cursor) {
    params.set('cursor_last_activity_at', String(cursor.lastActivityAt))
    params.set('cursor_id', cursor.id)
  }
  return await request<ProjectPage>(`/api/projects?${params}`)
}

export async function loadSidebarProjects(): Promise<Project[]> {
  return await request<Project[]>('/api/projects/sidebar')
}

export async function loadProject(projectId: string): Promise<Project | null> {
  return await request<Project | null>(`/api/projects/${encodeURIComponent(projectId)}`)
}

export async function loadProjectConversationMetas(
  projectId: string, cursor: ConversationCursor | null, limit = 20
): Promise<ConversationMetaPage> {
  const params = new URLSearchParams({ limit: String(limit) })
  if (cursor) {
    params.set('cursor_updated_at', String(cursor.updatedAt))
    params.set('cursor_id', cursor.id)
  }
  const items = await request<ConversationMeta[]>(
    `/api/projects/${encodeURIComponent(projectId)}/conversations?${params}`
  )
  return {
    items,
    nextCursor: items.length === limit && items.length
      ? { updatedAt: items[items.length - 1]!.updatedAt, id: items[items.length - 1]!.id }
      : null
  }
}

export async function createProject(name: string, workspaceRoot: string): Promise<ProjectCreationResult> {
  return await request<ProjectCreationResult>('/api/projects', {
    method: 'POST', body: JSON.stringify({ name, workspace_root: workspaceRoot })
  })
}

export async function createDirectory(_parentPath: string, _name: string): Promise<string> {
  throw new Error('当前网页环境不支持创建目录，请先在系统中创建目录后选择')
}

export async function updateProject(
  id: string, patch: Partial<Pick<Project, 'name' | 'workspaceRoot' | 'isPinned' | 'isArchived'>>
): Promise<Project> {
  return await request<Project>(`/api/projects/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify({
      name: patch.name, workspace_root: patch.workspaceRoot,
      is_pinned: patch.isPinned, is_archived: patch.isArchived
    })
  })
}

export async function deleteProject(id: string): Promise<void> {
  await request(`/api/projects/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

export async function searchConversations(
  query: string,
  limit = 50
): Promise<ConversationSearchHit[]> {
  const params = new URLSearchParams()
  params.set('q', query)
  params.set('limit', String(limit))
  return await request<ConversationSearchHit[]>(`/api/conversations/search?${params.toString()}`)
}

export async function listConversationSearchMatches(
  conversationId: string,
  query: string
): Promise<import('../types/chat').ConversationSearchMatch[]> {
  const params = new URLSearchParams()
  params.set('q', query)
  return await request(
    `/api/conversations/${encodeURIComponent(conversationId)}/search-matches?${params.toString()}`
  )
}

export async function listConversationOutline(
  conversationId: string
): Promise<import('../types/chat').ConversationOutlineItem[]> {
  return await request(
    `/api/conversations/${encodeURIComponent(conversationId)}/outline`
  )
}

export async function setMessageMilestone(
  conversationId: string,
  messageId: string,
  milestone: boolean
): Promise<void> {
  await request(
    `/api/conversations/${encodeURIComponent(conversationId)}/messages/${encodeURIComponent(messageId)}/milestone`,
    { method: 'POST', body: JSON.stringify({ milestone }) }
  )
}

export async function saveConversationMeta(metas: import('../types/chat').ConversationMeta[]): Promise<void> {
  await request('/api/conversations/meta', { method: 'PUT', body: JSON.stringify(metas) })
}

export async function deleteConversation(conversationId: string): Promise<void> {
  await request(`/api/conversations/${encodeURIComponent(conversationId)}`, { method: 'DELETE' })
}

export async function appendConversationMessages(
  conversationId: string,
  messages: import('../types/chat').ChatMessage[]
): Promise<AppendedMessageRow[]> {
  const appended = await request<AppendedMessageRow[]>(
    `/api/conversations/${encodeURIComponent(conversationId)}/messages/append`,
    { method: 'POST', body: JSON.stringify(messages) }
  )
  return appended ?? []
}

export interface PlatformSessionView {
  logged_in: boolean
  expires_at?: number | null
  user_nickname?: string | null
  /** Stable platform / local / SSO user id for per-user client prefs. */
  userId?: string | null
  isPlatformAdmin?: boolean
  includedTokens?: number
  consumedTokens?: number
  tokenQuotaExhausted?: boolean
}

export type AuthMode = 'platform' | 'standalone'

export async function getAuthMode(): Promise<AuthMode> {
  try {
    const res = await request<{ mode: string }>('/api/auth/mode')
    return res.mode === 'standalone' ? 'standalone' : 'platform'
  } catch (e) {
    console.warn('getAuthMode failed; defaulting to platform', e)
    return 'platform'
  }
}

export interface LocalCaptcha {
  captchaId: string
  imageSvg: string
}

export async function fetchLocalCaptcha(): Promise<LocalCaptcha> {
  return await request<LocalCaptcha>('/api/auth/local/captcha')
}

export async function localLogin(body: {
  username: string
  password: string
  captchaId: string
  captcha: string
}): Promise<PlatformSessionView> {
  return await request<PlatformSessionView>('/api/auth/local/login', {
    method: 'POST',
    body: JSON.stringify(body)
  })
}

export async function getPlatformSession(): Promise<PlatformSessionView> {
  return await request<PlatformSessionView>('/api/platform/session')
}

export async function openPlatformLogin(): Promise<void> {
  const res = await request<{ authorize_url: string }>('/api/auth/login/start', { method: 'POST' })
  // Redirect the browser away to the platform OAuth page; code after this
  // never runs. The SPA is reloaded by the server's callback redirect.
  window.location.href = res.authorize_url
}

export async function cancelPlatformLogin(): Promise<void> {
  // Web login is a browser redirect flow; there is no in-process loopback
  // listener to cancel. Users close the OAuth browser tab to abort.
}

export async function refreshPlatformSession(): Promise<PlatformSessionView> {
  return await request<PlatformSessionView>('/api/auth/refresh', { method: 'POST' })
}

export async function logoutPlatform(): Promise<void> {
  await request('/api/auth/logout', { method: 'POST' })
}

export async function loadPlatformSessionFromKeyring(): Promise<boolean> {
  // pointer-server: login is per-browser (HttpOnly cookie), not global auth.dat.
  const s = await refreshPlatformSession()
  return s.logged_in
}

export type TokenUsageListItem = import('./tauri').TokenUsageListItem
export type TokenUsageListResult = import('./tauri').TokenUsageListResult

export async function listTokenUsage(
  from?: string,
  to?: string
): Promise<TokenUsageListResult> {
  const params = new URLSearchParams()
  if (from) params.set('from', from)
  if (to) params.set('to', to)
  const qs = params.toString()
  return await request<TokenUsageListResult>(`/api/token-usage${qs ? `?${qs}` : ''}`)
}

/**
 * Subscribe to chat SSE. `onGap(reason)` fires when the browser must catch up
 * (broadcast lag, reconnect after drop, 502/504) — missed frames are not replayed.
 *
 * Resolves after the first successful SSE open (or timeout) so callers do not
 * POST /api/chat while the broadcast still has zero receivers.
 */
export async function onStream(
  handler: (e: StreamEvent) => void,
  conversationId = 'global',
  onGap?: (reason: string) => void
): Promise<() => void> {
  let stopped = false
  let abort: AbortController | null = null
  let connectGeneration = 0

  const notifyGap = (reason: string) => {
    console.warn('[stream] SSE gap — client should resync', { reason, conversationId })
    try {
      onGap?.(reason)
    } catch (err) {
      console.error('[stream] onGap handler failed', err)
    }
  }

  const parseSseChunk = (chunk: string) => {
    let eventName = 'message'
    const dataLines: string[] = []
    for (const line of chunk.split('\n')) {
      if (line.startsWith('event:')) {
        eventName = line.slice(6).trim()
      } else if (line.startsWith('data:')) {
        dataLines.push(line.slice(5).trimStart())
      }
    }
    if (eventName === 'resync') {
      notifyGap('server_lagged')
      return
    }
    if (eventName === 'keep-alive' || eventName === 'ping') return
    if (eventName !== 'message' || dataLines.length === 0) return
    const payload = dataLines.join('\n')
    if (!payload) return
    handler(JSON.parse(payload) as StreamEvent)
  }

  const connect = async () => {
    if (stopped) return
    const gen = ++connectGeneration
    abort = new AbortController()
    setChatStreamReady(false)
    try {
      const res = await fetch(
        `${WEB_API_BASE}/api/chat/${encodeURIComponent(conversationId)}/stream`,
        {
          credentials: 'include',
          signal: abort.signal,
          headers: { Accept: 'text/event-stream' }
        }
      )
      if (!res.ok || !res.body) {
        // 502/504 from nginx while the long-poll is open — always resync;
        // chat may already be `generating` even if no SSE frame arrived yet.
        throw new Error(`stream ${res.status}`)
      }
      console.info('[stream] SSE connected', { conversationId, generation: gen })
      setChatStreamReady(true)
      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      let receivedFrames = false
      while (!stopped) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })
        const parts = buffer.split('\n\n')
        buffer = parts.pop() ?? ''
        for (const part of parts) {
          if (!part.trim()) continue
          receivedFrames = true
          parseSseChunk(part)
        }
      }
      setChatStreamReady(false)
      if (!stopped && gen === connectGeneration) {
        // Proxy idle timeout / upstream close — catch up even if only keep-alives
        // were seen (Done may have been lost while the body stayed open).
        notifyGap(receivedFrames ? 'stream_ended' : 'stream_ended_idle')
        await new Promise(resolve => window.setTimeout(resolve, 1000))
        if (!stopped) void connect()
      }
    } catch (e) {
      setChatStreamReady(false)
      if (stopped || (e instanceof DOMException && e.name === 'AbortError')) return
      const msg = e instanceof Error ? e.message : String(e)
      console.warn('[stream] SSE error, reconnecting', msg)
      // Always resync: generating can be set via POST /api/chat before any SSE frame.
      notifyGap(/stream 50[234]/.test(msg) ? 'stream_gateway_error' : 'stream_error')
      await new Promise(resolve => window.setTimeout(resolve, 1500))
      if (!stopped) void connect()
    }
  }

  void connect()
  await waitForChatStreamReady(15_000)
  return () => {
    stopped = true
    setChatStreamReady(false)
    abort?.abort()
  }
}

/** True once the web SSE body is open (receivers exist for stream broadcast). */
let chatStreamReady = false
const chatStreamReadyWaiters: Array<() => void> = []

function setChatStreamReady(ready: boolean) {
  chatStreamReady = ready
  if (!ready) return
  const waiters = chatStreamReadyWaiters.splice(0, chatStreamReadyWaiters.length)
  for (const resolve of waiters) resolve()
}

/**
 * Wait until the global chat SSE is connected (or timeout).
 * New chats can POST /api/chat before the first SSE open; with zero receivers
 * the server drops frames and the reply only appears after refresh.
 */
export function waitForChatStreamReady(timeoutMs = 12_000): Promise<void> {
  if (chatStreamReady) return Promise.resolve()
  return new Promise(resolve => {
    const timer = window.setTimeout(() => {
      const idx = chatStreamReadyWaiters.indexOf(onReady)
      if (idx >= 0) chatStreamReadyWaiters.splice(idx, 1)
      console.warn('[stream] waitForChatStreamReady timed out', { timeoutMs })
      resolve()
    }, timeoutMs)
    const onReady = () => {
      window.clearTimeout(timer)
      resolve()
    }
    chatStreamReadyWaiters.push(onReady)
  })
}

// ---- Phase 5/6: automation (cron jobs + webhook token) ----

export async function getDispatcherQueueSnapshot(): Promise<
  import('../types/automation').RunQueueSnapshot
> {
  return await request('/api/dispatcher/queue')
}

export async function listCronJobs(): Promise<CronJob[]> {
  return await request<CronJob[]>('/api/cron-jobs')
}

export async function listCronDeliveryTargets(): Promise<CronDeliveryTarget[]> {
  return await request<CronDeliveryTarget[]>('/api/cron-jobs/delivery-targets')
}

export async function createCronJob(input: CreateCronJobInput): Promise<CronJob> {
  return await request<CronJob>('/api/cron-jobs', {
    method: 'POST',
    body: JSON.stringify(input)
  })
}

export async function updateCronJob(
  jobId: string,
  input: UpdateCronJobInput
): Promise<CronJob> {
  return await request<CronJob>(
    `/api/cron-jobs/${encodeURIComponent(jobId)}`,
    { method: 'PATCH', body: JSON.stringify(input) }
  )
}

export async function deleteCronJob(jobId: string): Promise<boolean> {
  // 204 → undefined; treat as success.
  await request<void>(`/api/cron-jobs/${encodeURIComponent(jobId)}`, {
    method: 'DELETE'
  })
  return true
}

export async function getWebhookConfig(): Promise<WebhookConfig> {
  return await request<WebhookConfig>('/api/webhooks/config')
}

export async function setWebhookSourceToken(
  src: string,
  token: string,
  authHeaderName?: string | null,
  sessionMode?: import('../types/automation').WebhookSessionMode
): Promise<WebhookConfig> {
  return await request<WebhookConfig>('/api/webhooks/config', {
    method: 'POST',
    body: JSON.stringify({ src, token, authHeaderName, sessionMode })
  })
}

export async function patchWebhookSource(
  src: string,
  patch: { sessionMode?: import('../types/automation').WebhookSessionMode }
): Promise<WebhookConfig> {
  return await request<WebhookConfig>(`/api/webhooks/config/${encodeURIComponent(src)}`, {
    method: 'PATCH',
    body: JSON.stringify(patch)
  })
}

export async function clearWebhookSourceToken(src: string): Promise<boolean> {
  await request<void>(`/api/webhooks/config/${encodeURIComponent(src)}`, { method: 'DELETE' })
  return true
}

export async function clearWebhookLegacyToken(): Promise<boolean> {
  await request<void>('/api/webhooks/config/legacy', { method: 'DELETE' })
  return true
}

export async function revealWebhookSourceToken(
  src: string
): Promise<import('../types/automation').WebhookTokenReveal> {
  return await request<import('../types/automation').WebhookTokenReveal>(
    `/api/webhooks/config/${encodeURIComponent(src)}/token`
  )
}
