import type {
  AgentDef,
  AgentMode,
  ChatMessage,
  ComputerAnnotatedPreview,
  ChatMediaPreview,
  ComputerMonitor,
  Conversation,
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

import { WEB_API_BASE } from './runtime'

/** Windows 上连接未监听端口时，fetch 可能长时间挂起；超时后尽快失败以便界面可用。 */
const REQUEST_TIMEOUT_MS = 12_000

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentMode?: AgentMode
  toolRoundsUsed?: number
  toolRoundsUsedSupervisor?: number
  workspaceRoot?: string
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

export async function sendChat(payload: SendChatPayload): Promise<void> {
  await request('/api/chat', { method: 'POST', body: JSON.stringify(payload) })
}

export async function cancelChat(conversationId: string): Promise<void> {
  await request(`/api/chat/${encodeURIComponent(conversationId)}/cancel`, { method: 'POST' })
}

export async function abortTerminalCommand(conversationId: string): Promise<boolean> {
  const j = await request<{ aborted: boolean }>(
    `/api/chat/${encodeURIComponent(conversationId)}/abort-terminal`,
    { method: 'POST' }
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

export async function getSettings(): Promise<EffectiveSettingsView> {
  return await request<EffectiveSettingsView>('/api/settings')
}

export async function updateSettings(settings: ModelSettings): Promise<EffectiveSettingsView> {
  return await request<EffectiveSettingsView>('/api/settings', { method: 'PUT', body: JSON.stringify(settings) })
}

export async function updateAgentSettings(settings: ModelSettings): Promise<EffectiveSettingsView> {
  return await request<EffectiveSettingsView>('/api/agent-settings', { method: 'PUT', body: JSON.stringify(settings) })
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

export async function openChatMedia(_storageRelPath: string): Promise<void> {
  throw new Error('openChatMedia is not supported in web runtime')
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
  const convs = await loadConversations()
  return convs.find(c => c.id === conversationId)?.messages ?? []
}

export async function loadConversations(): Promise<Conversation[]> {
  return await request<Conversation[]>('/api/conversations')
}

export async function saveConversations(conversations: Conversation[]): Promise<void> {
  await request('/api/conversations', { method: 'PUT', body: JSON.stringify(conversations) })
}

export async function saveConversationMeta(metas: import('../types/chat').ConversationMeta[]): Promise<void> {
  await request('/api/conversations/meta', { method: 'PUT', body: JSON.stringify(metas) })
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

export async function getPlatformSession(): Promise<PlatformSessionView> {
  return { logged_in: false }
}

export async function openPlatformLogin(): Promise<void> {
  throw new Error('Web 端平台登录请使用桌面客户端')
}

export async function cancelPlatformLogin(): Promise<void> {}

export async function refreshPlatformSession(): Promise<PlatformSessionView> {
  return { logged_in: false }
}

export async function logoutPlatform(): Promise<void> {}

export async function loadPlatformSessionFromKeyring(): Promise<boolean> {
  return false
}

export async function onStream(handler: (e: StreamEvent) => void, conversationId = 'global'): Promise<() => void> {
  let source: EventSource | null = null
  let stopped = false

  const connect = () => {
    if (stopped) return
    source = new EventSource(`${WEB_API_BASE}/api/chat/${encodeURIComponent(conversationId)}/stream`)
    source.onmessage = ev => handler(JSON.parse(ev.data) as StreamEvent)
    source.onerror = () => {
      source?.close()
      source = null
      if (!stopped) window.setTimeout(connect, 1000)
    }
  }

  connect()
  return () => {
    stopped = true
    source?.close()
  }
}
