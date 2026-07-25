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
  Project,
  ProjectPage,
  ConversationSearchHit,
  DebugSessionSettings,
  EffectiveSettingsView,
  MediaDepsStatus,
  ModelSettings,
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

/** Windows 上连接未监听端口时，fetch 可能长时间挂起；超时后尽快失败以便界面可用。 */
const REQUEST_TIMEOUT_MS = 12_000

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
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  if (init?.body && !(init.body instanceof FormData) && !(init.body instanceof Blob) && !(init.body instanceof ArrayBuffer)) {
    headers.set('Content-Type', 'application/json')
  }

  const controller = new AbortController()
  const timeoutId = window.setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS)
  let res: Response
  try {
    res = await fetch(`${WEB_API_BASE}${path}`, {
      ...init,
      headers,
      credentials: 'include',
      signal: init?.signal ?? controller.signal
    })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      throw new Error(
        `请求超时（>${REQUEST_TIMEOUT_MS / 1000}s）：${WEB_API_BASE} 无响应。请确认已启动 pointer-server（默认 127.0.0.1:8787）或设置 VITE_WEB_API_BASE。`
      )
    }
    throw e
  } finally {
    window.clearTimeout(timeoutId)
  }

  if (!res.ok) throw new Error(await res.text())
  if (res.status === 204 || res.status === 202) return undefined as T
  return await res.json()
}

async function requestBlob(path: string, init?: RequestInit): Promise<Blob> {
  const controller = new AbortController()
  const timeoutId = window.setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS)
  let res: Response
  try {
    res = await fetch(`${WEB_API_BASE}${path}`, {
      ...init,
      credentials: 'include',
      signal: init?.signal ?? controller.signal
    })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      throw new Error(
        `请求超时（>${REQUEST_TIMEOUT_MS / 1000}s）：${WEB_API_BASE} 无响应。请确认已启动 pointer-server（默认 127.0.0.1:8787）或设置 VITE_WEB_API_BASE。`
      )
    }
    throw e
  } finally {
    window.clearTimeout(timeoutId)
  }

  if (!res.ok) throw new Error(await res.text())
  return await res.blob()
}

function downloadBlob(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = fileName || 'attachment'
  anchor.rel = 'noopener'
  anchor.click()
  window.setTimeout(() => URL.revokeObjectURL(url), 60_000)
}

export async function sendChat(payload: SendChatPayload): Promise<void> {
  await request('/api/chat', { method: 'POST', body: JSON.stringify(payload) })
}

export async function cancelChat(conversationId: string): Promise<void> {
  await request(`/api/chat/${encodeURIComponent(conversationId)}/cancel`, { method: 'POST' })
}

function workspaceQuery(workspaceRoot: string, relativePath?: string): string {
  const params = new URLSearchParams({ workspaceRoot })
  if (relativePath !== undefined) params.set('relativePath', relativePath)
  return params.toString()
}

export async function listWorkspaceDirectory(
  workspaceRoot: string,
  relativePath?: string
): Promise<import('./api').WorkspaceEntry[]> {
  return await request(`/api/workspace/directory?${workspaceQuery(workspaceRoot, relativePath)}`)
}

export async function readWorkspaceFile(
  workspaceRoot: string,
  relativePath: string
): Promise<import('./api').WorkspaceFilePreview> {
  return await request(`/api/workspace/file?${workspaceQuery(workspaceRoot, relativePath)}`)
}

export async function getWorkspaceGitStatus(workspaceRoot: string): Promise<import('./api').GitStatusResponse> {
  return await request(`/api/workspace/git/status?${workspaceQuery(workspaceRoot)}`)
}

export async function getWorkspaceGitDiff(
  workspaceRoot: string,
  relativePath: string
): Promise<import('./api').GitDiff> {
  return await request(`/api/workspace/git/diff?${workspaceQuery(workspaceRoot, relativePath)}`)
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

export async function updateSettings(settings: ModelSettings): Promise<EffectiveSettingsView> {
  return await request<EffectiveSettingsView>('/api/settings', { method: 'PUT', body: JSON.stringify(settings) })
}

export async function updateAgentSettings(settings: ModelSettings): Promise<EffectiveSettingsView> {
  return await request<EffectiveSettingsView>('/api/agent-settings', { method: 'PUT', body: JSON.stringify(settings) })
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
  payload: import('./api').SaveChatAttachmentPayload
): Promise<string> {
  const res = await request<{ storageRelPath: string }>('/api/chat/save-attachment', {
    method: 'POST',
    body: JSON.stringify(payload)
  })
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

/** Authenticated download (includes session cookie; plain `<a href>` does not). */
export async function downloadChatMedia(
  storageRelPath: string,
  fileName?: string
): Promise<void> {
  const q = new URLSearchParams({ storageRelPath: storageRelPath.trim() })
  const blob = await requestBlob(`/api/chat/media-download?${q}`)
  downloadBlob(blob, fileName?.trim() || 'attachment')
}

export async function downloadChatMediaRef(mediaRef: string, fileName?: string): Promise<void> {
  const q = new URLSearchParams({ mediaRef: mediaRef.trim() })
  const blob = await requestBlob(`/api/chat/media-ref-download?${q}`)
  downloadBlob(blob, fileName?.trim() || 'attachment')
}

/** Inline video preview URL with auth (object URL; revoke when the element unmounts). */
export async function chatMediaStreamObjectUrl(storageRelPath: string): Promise<string> {
  const q = new URLSearchParams({ storageRelPath: storageRelPath.trim() })
  const blob = await requestBlob(`/api/chat/media-stream?${q}`)
  return URL.createObjectURL(blob)
}

export function chatMediaDownloadUrl(storageRelPath: string): string {
  const q = encodeURIComponent(storageRelPath.trim())
  return `${WEB_API_BASE}/api/chat/media-download?storageRelPath=${q}`
}

export function chatMediaStreamUrl(storageRelPath: string): string {
  const q = encodeURIComponent(storageRelPath.trim())
  return `${WEB_API_BASE}/api/chat/media-stream?storageRelPath=${q}`
}

export function chatMediaRefDownloadUrl(mediaRef: string): string {
  const q = new URLSearchParams({ mediaRef: mediaRef.trim() })
  return `${WEB_API_BASE}/api/chat/media-ref-download?${q}`
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

export async function loadConversations(): Promise<Conversation[]> {
  return await request<Conversation[]>('/api/conversations')
}

/** Cursor-paginated meta-only list (no messages). Sort: updatedAt DESC, id DESC. */
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

export async function loadProjects(cursor: ConversationCursor | null, limit = 20): Promise<ProjectPage> {
  const params = new URLSearchParams({ limit: String(limit) })
  if (cursor) {
    params.set('cursor_updated_at', String(cursor.updatedAt))
    params.set('cursor_id', cursor.id)
  }
  return await request<ProjectPage>(`/api/projects?${params}`)
}

export async function loadSidebarProjects(): Promise<Project[]> {
  return await request<Project[]>('/api/projects/sidebar')
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

export async function createProject(name: string, workspaceRoot: string): Promise<Project> {
  return await request<Project>('/api/projects', {
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

export async function saveConversations(conversations: Conversation[]): Promise<void> {
  await request('/api/conversations', { method: 'PUT', body: JSON.stringify(conversations) })
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
): Promise<number> {
  const written = await request<number>(
    `/api/conversations/${encodeURIComponent(conversationId)}/messages/append`,
    { method: 'POST', body: JSON.stringify(messages) }
  )
  return written ?? 0
}

export interface PlatformSessionView {
  logged_in: boolean
  expires_at?: number | null
  user_nickname?: string | null
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

export async function onStream(handler: (e: StreamEvent) => void, conversationId = 'global'): Promise<() => void> {
  let stopped = false
  let abort: AbortController | null = null

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
    if (eventName !== 'message' || dataLines.length === 0) return
    const payload = dataLines.join('\n')
    if (!payload) return
    handler(JSON.parse(payload) as StreamEvent)
  }

  const connect = async () => {
    if (stopped) return
    abort = new AbortController()
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
        throw new Error(`stream ${res.status}`)
      }
      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      while (!stopped) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })
        const parts = buffer.split('\n\n')
        buffer = parts.pop() ?? ''
        for (const part of parts) {
          if (part.trim()) parseSseChunk(part)
        }
      }
    } catch (e) {
      if (stopped || (e instanceof DOMException && e.name === 'AbortError')) return
      await new Promise(resolve => window.setTimeout(resolve, 1000))
      if (!stopped) void connect()
    }
  }

  void connect()
  return () => {
    stopped = true
    abort?.abort()
  }
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
