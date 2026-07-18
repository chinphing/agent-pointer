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
  ConversationCursor,
  ConversationMeta,
  ConversationMetaPage,
  ConversationSearchHit,
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
  UpdateCronJobInput,
  WebhookConfig,
  WebhookTokenReveal
} from '../types/automation'

export const STREAM_EVENT = 'chat://stream'

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
  leadAgentId?: string
}

export async function sendChat(payload: SendChatPayload): Promise<string> {
  return await invoke<string>('send_chat', { payload })
}

export async function cancelChat(conversationId: string): Promise<void> {
  await invoke('cancel_chat', { conversationId })
}

export async function abortTerminalCommand(
  conversationId: string,
  toolCallId?: string
): Promise<boolean> {
  return await invoke<boolean>('abort_terminal_command', {
    conversationId,
    toolCallId: toolCallId ?? null
  })
}

export async function approveToolCall(
  _conversationId: string,
  toolCallId: string,
  approved: boolean
): Promise<void> {
  await invoke('approve_tool_call', { toolCallId, approved })
}

export async function submitTerminalInput(requestId: string, text: string): Promise<void> {
  await invoke('submit_terminal_input', { requestId, text })
}

export async function dismissTerminalInput(requestId: string): Promise<void> {
  await invoke('dismiss_terminal_input', { requestId })
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

/** Cursor-paginated meta-only list (no messages). Sort: updatedAt DESC, id DESC. */
export async function loadConversationMetas(
  cursor: ConversationCursor | null,
  limit = 50
): Promise<ConversationMetaPage> {
  const items = await invoke<ConversationMeta[]>('load_conversation_metas', {
    cursorUpdatedAt: cursor?.updatedAt ?? null,
    cursorId: cursor?.id ?? null,
    limit
  })
  const nextCursor =
    items.length === limit && items.length > 0
      ? { updatedAt: items[items.length - 1]!.updatedAt, id: items[items.length - 1]!.id }
      : null
  return { items, nextCursor }
}

export async function searchConversations(
  query: string,
  limit = 50
): Promise<ConversationSearchHit[]> {
  return await invoke<ConversationSearchHit[]>('search_conversations', { query, limit })
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

export async function deleteConversation(conversationId: string): Promise<void> {
  await invoke('delete_conversation', { conversationId })
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

export async function openPlatformLogin(): Promise<PlatformSessionView> {
  return await invoke<PlatformSessionView>('open_platform_login')
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

// ---- Phase 5/6: automation (cron jobs + webhook token) ----

export async function getDispatcherQueueSnapshot(): Promise<
  import('../types/automation').RunQueueSnapshot
> {
  return await invoke('get_dispatcher_queue_snapshot')
}

export async function listCronJobs(): Promise<CronJob[]> {
  return await invoke<CronJob[]>('list_cron_jobs')
}

export async function createCronJob(input: CreateCronJobInput): Promise<CronJob> {
  // Tauri command args are received as a camelCase struct on the Rust side
  // (`CreateCronJobArgs`); invoke passes the object through unchanged.
  return await invoke<CronJob>('create_cron_job', { args: input })
}

export async function updateCronJob(
  jobId: string,
  input: UpdateCronJobInput
): Promise<CronJob> {
  return await invoke<CronJob>('update_cron_job', { jobId, args: input })
}

export async function deleteCronJob(jobId: string): Promise<boolean> {
  return await invoke<boolean>('delete_cron_job', { jobId })
}

export async function getWebhookConfig(): Promise<WebhookConfig> {
  return await invoke<WebhookConfig>('get_webhook_config')
}

export async function setWebhookSourceToken(
  src: string,
  token: string,
  authHeaderName?: string | null,
  sessionMode?: import('../types/automation').WebhookSessionMode
): Promise<WebhookConfig> {
  return await invoke<WebhookConfig>('set_webhook_source_token', {
    src,
    token,
    authHeaderName,
    sessionMode
  })
}

export async function patchWebhookSource(
  src: string,
  patch: { sessionMode?: import('../types/automation').WebhookSessionMode }
): Promise<WebhookConfig> {
  return await invoke<WebhookConfig>('patch_webhook_source', {
    src,
    sessionMode: patch.sessionMode
  })
}

export async function clearWebhookSourceToken(src: string): Promise<boolean> {
  return await invoke<boolean>('clear_webhook_source_token', { src })
}

export async function clearWebhookLegacyToken(): Promise<boolean> {
  return await invoke<boolean>('clear_webhook_legacy_token')
}

export async function revealWebhookSourceToken(
  src: string
): Promise<WebhookTokenReveal> {
  return await invoke<WebhookTokenReveal>('reveal_webhook_source_token', { src })
}
