import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AgentDef,
  AgentMode,
  ChatMessage,
  ComputerAnnotatedPreview,
  ComputerMonitor,
  Conversation,
  ModelSettings,
  SkillDef,
  SkillImportResult,
  StreamEvent,
  ToolDef
} from '../types/chat'

export const STREAM_EVENT = 'chat://stream'

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentMode?: AgentMode
  toolRoundsUsed?: number
  toolRoundsUsedSupervisor?: number
}

export async function sendChat(payload: SendChatPayload): Promise<string> {
  return await invoke<string>('send_chat', { payload })
}

export async function cancelChat(conversationId: string): Promise<void> {
  await invoke('cancel_chat', { conversationId })
}

export async function approveToolCall(
  _conversationId: string,
  toolCallId: string,
  approved: boolean
): Promise<void> {
  await invoke('approve_tool_call', { toolCallId, approved })
}

export async function getSettings(): Promise<ModelSettings> {
  return await invoke<ModelSettings>('get_settings')
}

export async function updateSettings(settings: ModelSettings): Promise<ModelSettings> {
  return await invoke<ModelSettings>('update_settings', { settings })
}

export async function setApiKey(key: string): Promise<void> {
  await invoke('set_api_key', { apiKey: key })
}

export async function clearApiKey(): Promise<void> {
  await invoke('clear_api_key')
}

export async function testConnection(): Promise<{ ok: boolean; latencyMs: number; message: string }> {
  try {
    const ms = await invoke<number>('test_connection')
    return { ok: true, latencyMs: Number(ms), message: '连接成功' }
  } catch (e: any) {
    return { ok: false, latencyMs: 0, message: String(e?.message || e) }
  }
}

export async function listSkills(): Promise<SkillDef[]> {
  return await invoke<SkillDef[]>('list_skills')
}

export async function importSkillZip(file: File): Promise<SkillImportResult> {
  const data = Array.from(new Uint8Array(await file.arrayBuffer()))
  return await invoke<SkillImportResult>('import_skill_zip', { zipData: data })
}

export async function listTools(): Promise<ToolDef[]> {

  return await invoke<ToolDef[]>('list_tools')
}

export async function listAgents(): Promise<AgentDef[]> {
  return await invoke<AgentDef[]>('list_agents')
}

export async function previewComputerAnnotatedScreen(): Promise<ComputerAnnotatedPreview> {
  return await invoke<ComputerAnnotatedPreview>('preview_computer_annotated_screen')
}

export async function previewComputerRoundScreen(relPath: string): Promise<ComputerAnnotatedPreview> {
  return await invoke<ComputerAnnotatedPreview>('preview_computer_round_screen', { relPath })
}

export async function listComputerMonitors(): Promise<ComputerMonitor[]> {
  return await invoke<ComputerMonitor[]>('list_computer_monitors')
}

export async function setComputerConversationMonitor(conversationId: string, monitorId: string | null): Promise<void> {
  await invoke('set_computer_conversation_monitor', {
    conversationId,
    monitorId: monitorId === '' ? null : monitorId
  })
}

export async function loadConversations(): Promise<Conversation[]> {
  return await invoke<Conversation[]>('load_conversations')
}

export async function saveConversations(conversations: Conversation[]): Promise<void> {
  await invoke('save_conversations', { conversations })
}

export async function onStream(handler: (e: StreamEvent) => void): Promise<UnlistenFn> {
  return await listen<StreamEvent>(STREAM_EVENT, ev => handler(ev.payload))
}
