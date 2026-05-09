import type { AgentDef, AgentMode, ChatMessage, Conversation, ModelSettings, SkillDef, SkillImportResult, StreamEvent, ToolDef } from '../types/chat'

import { WEB_API_BASE } from './runtime'

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentMode?: AgentMode
  toolRoundsUsed?: number
  toolRoundsUsedSupervisor?: number
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  if (init?.body && !(init.body instanceof FormData) && !(init.body instanceof Blob) && !(init.body instanceof ArrayBuffer)) {
    headers.set('Content-Type', 'application/json')
  }

  const res = await fetch(`${WEB_API_BASE}${path}`, {
    ...init,
    headers
  })

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

export async function getSettings(): Promise<ModelSettings> {
  return await request<ModelSettings>('/api/settings')
}

export async function updateSettings(settings: ModelSettings): Promise<ModelSettings> {
  return await request<ModelSettings>('/api/settings', { method: 'PUT', body: JSON.stringify(settings) })
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

export async function importSkillZip(file: File): Promise<SkillImportResult> {
  return await request<SkillImportResult>('/api/skills', {
    method: 'POST',
    body: await file.arrayBuffer()
  })
}

export async function listTools(): Promise<ToolDef[]> {

  return await request<ToolDef[]>('/api/tools')
}

export async function listAgents(): Promise<AgentDef[]> {
  return await request<AgentDef[]>('/api/agents')
}

export async function loadConversations(): Promise<Conversation[]> {
  return await request<Conversation[]>('/api/conversations')
}

export async function saveConversations(conversations: Conversation[]): Promise<void> {
  await request('/api/conversations', { method: 'PUT', body: JSON.stringify(conversations) })
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
