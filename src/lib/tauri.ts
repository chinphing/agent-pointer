import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AgentDef,
  AgentMode,
  ChatMessage,
  ComputerAnnotatedPreview,
  ChatMediaPreview,
  ComputerMonitor,
  Conversation,
  ConversationMeta,
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

export const STREAM_EVENT = 'chat://stream'

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentMode?: AgentMode
  toolRoundsUsed?: number
  toolRoundsUsedSupervisor?: number
  workspaceRoot?: string
  workspaceInheritDisabled?: boolean
  leadAgentId?: string
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

export async function probeExternalSkills(): Promise<ExternalSkillsProbeResult> {
  return await invoke<ExternalSkillsProbeResult>('probe_external_skills')
}

export async function importExternalSkills(sourceIds: string[]): Promise<SkillImportResult> {
  return await invoke<SkillImportResult>('import_external_skills', { sourceIds })
}

export async function dismissExternalSkillsPrompt(): Promise<void> {
  await invoke('dismiss_external_skills_prompt')
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

export async function listWorkItems(
  conversationId: string,
  opts?: { taskId?: string; batchId?: string; offset?: number; limit?: number }
): Promise<Record<string, unknown>> {
  return await invoke('list_work_items', {
    conversationId,
    taskId: opts?.taskId ?? null,
    batchId: opts?.batchId ?? null,
    offset: opts?.offset ?? null,
    limit: opts?.limit ?? null
  })
}

export async function getWorkItemStats(
  conversationId: string,
  opts?: { taskId?: string; batchId?: string }
): Promise<Record<string, unknown>> {
  return await invoke('work_item_stats', {
    conversationId,
    taskId: opts?.taskId ?? null,
    batchId: opts?.batchId ?? null
  })
}

export async function previewComputerAnnotatedScreen(conversationId: string): Promise<ComputerAnnotatedPreview> {
  return await invoke<ComputerAnnotatedPreview>('preview_computer_annotated_screen', { conversationId })
}

export async function previewComputerRoundScreen(relPath: string): Promise<ComputerAnnotatedPreview> {
  return await invoke<ComputerAnnotatedPreview>('preview_computer_round_screen', { relPath })
}

export async function previewChatMedia(storageRelPath: string): Promise<ChatMediaPreview> {
  return await invoke<ChatMediaPreview>('preview_chat_media', { storageRelPath })
}

export async function getChatMediaLocalPath(storageRelPath: string): Promise<string> {
  return await invoke<string>('get_chat_media_local_path', { storageRelPath })
}

export async function previewMediaRef(mediaRef: string): Promise<ChatMediaPreview> {
  return await invoke<ChatMediaPreview>('preview_media_ref', { mediaRef })
}

export async function saveChatAttachment(payload: import('./api').SaveChatAttachmentPayload): Promise<string> {
  return await invoke<string>('save_chat_attachment', {
    conversationId: payload.conversationId,
    attachmentId: payload.attachmentId,
    contentBase64: payload.contentBase64,
    fileName: payload.fileName
  })
}

export async function checkMediaDeps(): Promise<MediaDepsStatus> {
  return await invoke<MediaDepsStatus>('check_media_deps')
}

export async function listComputerMonitors(): Promise<ComputerMonitor[]> {
  return await invoke<ComputerMonitor[]>('list_computer_monitors')
}

export async function setComputerCompactChrome(compact: boolean): Promise<void> {
  await invoke('set_computer_compact_chrome', { compact })
}

export async function beginComputerCompactWindow(): Promise<void> {
  await invoke('begin_computer_compact_window')
}

export async function restoreComputerCompactWindowNative(): Promise<void> {
  await invoke('restore_computer_compact_window')
}

export async function placeComputerCompactWindow(
  width: number,
  height: number,
  margin: number
): Promise<void> {
  await invoke('place_computer_compact_window', { width, height, margin })
}

export async function reapplyWindowChrome(): Promise<void> {
  await invoke('reapply_window_chrome')
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

export async function loadConversationMessages(conversationId: string): Promise<ChatMessage[]> {
  return await invoke<ChatMessage[]>('load_conversation_messages', { conversationId })
}

export async function saveConversations(conversations: Conversation[]): Promise<void> {
  await invoke('save_conversations', { conversations })
}

export async function saveConversationMeta(metas: ConversationMeta[]): Promise<void> {
  await invoke('save_conversation_meta', { metas })
}

export async function appendConversationMessages(
  conversationId: string,
  messages: ChatMessage[]
): Promise<number> {
  return await invoke<number>('append_conversation_messages', { conversationId, messages })
}

export async function onStream(handler: (e: StreamEvent) => void): Promise<UnlistenFn> {
  return await listen<StreamEvent>(STREAM_EVENT, ev => handler(ev.payload))
}

export async function revealInFinder(path: string): Promise<void> {
  await invoke('reveal_in_finder', { path })
}

export async function openPathWithDefaultApp(path: string): Promise<void> {
  await invoke('open_path_with_default_app', { path })
}

export async function openChatMedia(storageRelPath: string): Promise<void> {
  await invoke('open_chat_media', { storageRelPath })
}

export async function getLocalFileSize(path: string): Promise<number> {
  return await invoke<number>('get_local_file_size', { path })
}

export async function readLocalFileForAttachment(
  path: string
): Promise<import('./api').LocalFileAttachmentPayload> {
  const raw = await invoke<{
    file_name: string
    mime_type: string
    size_bytes: number
    content_base64: string
  }>('read_local_file_for_attachment', { path })
  return {
    fileName: raw.file_name,
    mimeType: raw.mime_type,
    sizeBytes: raw.size_bytes,
    contentBase64: raw.content_base64
  }
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
