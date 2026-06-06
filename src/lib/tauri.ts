import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AgentDef,
  AgentMode,
  ChatMessage,
  ComputerAnnotatedPreview,
  ComputerMonitor,
  Conversation,
  EffectiveSettingsView,
  ModelSettings,
  PlatformSettings,
  SkillDef,
  SkillImportResult,
  StreamEvent,
  ToolDef,
  UserSettings
} from '../types/chat'

export const STREAM_EVENT = 'chat://stream'

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentMode?: AgentMode
  toolRoundsUsed?: number
  toolRoundsUsedSupervisor?: number
  workspaceRoot?: string
}

export async function sendChat(payload: SendChatPayload): Promise<string> {
  return await invoke<string>('send_chat', { payload })
}

export async function cancelChat(conversationId: string): Promise<void> {
  await invoke('cancel_chat', { conversationId })
}

export async function abortTerminalCommand(conversationId: string): Promise<boolean> {
  return await invoke<boolean>('abort_terminal_command', { conversationId })
}

export async function approveToolCall(
  _conversationId: string,
  toolCallId: string,
  approved: boolean
): Promise<void> {
  await invoke('approve_tool_call', { toolCallId, approved })
}

export async function getSettings(): Promise<EffectiveSettingsView> {
  return await invoke<EffectiveSettingsView>('get_settings')
}

export async function updateSettings(settings: ModelSettings): Promise<EffectiveSettingsView> {
  return await invoke<EffectiveSettingsView>('update_settings', { settings })
}

export async function updateAgentSettings(settings: ModelSettings): Promise<EffectiveSettingsView> {
  return await invoke<EffectiveSettingsView>('update_agent_settings', { settings })
}

export async function updateUserSettings(user: UserSettings): Promise<EffectiveSettingsView> {
  return await invoke<EffectiveSettingsView>('update_user_settings', { user })
}

export async function updatePlatformSettings(platform: PlatformSettings): Promise<EffectiveSettingsView> {
  return await invoke<EffectiveSettingsView>('update_platform_settings', { platform })
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

export async function reloadSkillMeta(): Promise<SkillDef[]> {
  return await invoke<SkillDef[]>('reload_skill_meta')
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

export async function getTaskBoardSnapshot(
  conversationId: string,
  taskId?: string
): Promise<import('../types/chat').TaskBoardDocument> {
  return await invoke('get_task_board_snapshot', { conversationId, taskId: taskId ?? null })
}

export async function previewComputerAnnotatedScreen(conversationId: string): Promise<ComputerAnnotatedPreview> {
  return await invoke<ComputerAnnotatedPreview>('preview_computer_annotated_screen', { conversationId })
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

export async function confirmComputerMonitorPick(conversationId: string): Promise<void> {
  await invoke('confirm_computer_monitor_pick', { conversationId })
}

export async function cancelComputerMonitorPick(conversationId: string): Promise<void> {
  await invoke('cancel_computer_monitor_pick', { conversationId })
}

export type { MacosComputerPermissionsStatus } from '../types/macosPermissions'

export async function getMacosComputerPermissions(): Promise<import('../types/macosPermissions').MacosComputerPermissionsStatus> {
  return await invoke('get_macos_computer_permissions')
}

/** Register current process in Screen Recording settings (call when permission wizard opens). */
export async function registerMacosScreenRecordingAccess(): Promise<void> {
  await invoke('register_macos_screen_recording_access')
}

export type MacosPermissionDragKind = 'screenRecording' | 'accessibility'

export async function openMacosComputerPermissionSettings(kind: MacosPermissionDragKind): Promise<void> {
  await invoke('open_macos_computer_permission_settings', { kind })
}

export async function beginMacosPermissionDragFlow(kind: MacosPermissionDragKind): Promise<void> {
  await invoke('begin_macos_permission_drag_flow', { kind })
}

export async function dismissMacosPermissionDragGuide(): Promise<void> {
  await invoke('dismiss_macos_permission_drag_guide')
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
  return await invoke<PlatformSessionView>('get_platform_session')
}

export async function openPlatformLogin(): Promise<void> {
  await invoke('open_platform_login')
}

export async function cancelPlatformLogin(): Promise<void> {
  await invoke('cancel_platform_login')
}

export async function refreshPlatformSession(): Promise<PlatformSessionView> {
  return await invoke<PlatformSessionView>('refresh_platform_session')
}

export async function logoutPlatform(): Promise<void> {
  await invoke('logout_platform')
}

export async function loadPlatformSessionPersisted(): Promise<boolean> {
  return await invoke<boolean>('load_platform_session_persisted')
}

export async function loadPlatformSessionFromKeyring(): Promise<boolean> {
  return await invoke<boolean>('load_platform_session_from_keyring')
}
