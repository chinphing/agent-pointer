import type { UnlistenFn } from '@tauri-apps/api/event'
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
  StreamEvent,
  ToolDef,
  UserSettings
} from '../types/chat'

import * as tauriApi from './tauri'
import * as webApi from './web'
import { isTauriRuntime } from './runtime'

export interface SaveChatAttachmentPayload {
  conversationId: string
  attachmentId: string
  contentBase64: string
  fileName: string
}

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentMode?: AgentMode
  /** Cumulative single-agent tool rounds before this send. */
  toolRoundsUsed?: number
  /** Cumulative Supervisor/sub-agent tool rounds before this send. */
  toolRoundsUsedSupervisor?: number
  /** Per-conversation workspace root for this run. */
  workspaceRoot?: string
}

export interface RuntimeApi {
  sendChat(payload: SendChatPayload): Promise<string | void>
  cancelChat(conversationId: string): Promise<void>
  /** Stops only the in-flight `terminal` subprocess; the chat turn continues. */
  abortTerminalCommand(conversationId: string): Promise<boolean>
  approveToolCall(conversationId: string, toolCallId: string, approved: boolean): Promise<void>
  getSettings(): Promise<EffectiveSettingsView>
  updateSettings(settings: ModelSettings): Promise<EffectiveSettingsView>
  updateAgentSettings(settings: ModelSettings): Promise<EffectiveSettingsView>
  updateUserSettings(user: UserSettings): Promise<EffectiveSettingsView>
  updatePlatformSettings(platform: PlatformSettings): Promise<EffectiveSettingsView>
  setApiKey(key: string): Promise<void>
  clearApiKey(): Promise<void>
  testConnection(): Promise<{ ok: boolean; latencyMs: number; message: string }>
  listSkills(): Promise<SkillDef[]>
  reloadSkillMeta(): Promise<SkillDef[]>
  importSkillZip(file: File): Promise<SkillImportResult>
  probeExternalSkills(): Promise<import('../types/chat').ExternalSkillsProbeResult>
  importExternalSkills(sourceIds: string[]): Promise<SkillImportResult>
  dismissExternalSkillsPrompt(): Promise<void>
  listTools(): Promise<ToolDef[]>
  listAgents(): Promise<AgentDef[]>
  getTaskBoardSnapshot(conversationId: string, taskId?: string): Promise<import('../types/chat').TaskBoardDocument>
  previewComputerAnnotatedScreen(conversationId: string): Promise<ComputerAnnotatedPreview>
  previewComputerRoundScreen(relPath: string): Promise<ComputerAnnotatedPreview>
  previewChatMedia(storageRelPath: string): Promise<ChatMediaPreview>
  previewMediaRef(mediaRef: string): Promise<ChatMediaPreview>
  saveChatAttachment(payload: SaveChatAttachmentPayload): Promise<string>
  checkMediaDeps(): Promise<MediaDepsStatus>
  listComputerMonitors(): Promise<ComputerMonitor[]>
  setComputerCompactChrome(compact: boolean): Promise<void>
  reapplyWindowChrome(): Promise<void>
  setComputerConversationMonitor(conversationId: string, monitorId: string | null): Promise<void>
  confirmComputerMonitorPick(conversationId: string): Promise<void>
  cancelComputerMonitorPick(conversationId: string): Promise<void>
  getMacosComputerPermissions?(): Promise<import('../types/macosPermissions').MacosComputerPermissionsStatus>
  openMacosComputerPermissionSettings?(kind: import('./tauri').MacosPermissionDragKind): Promise<void>
  beginMacosPermissionDragFlow?(kind: import('./tauri').MacosPermissionDragKind): Promise<void>
  dismissMacosPermissionDragGuide?(): Promise<void>

  loadConversations(): Promise<Conversation[]>
  saveConversations(conversations: Conversation[]): Promise<void>
  revealInFinder(path: string): Promise<void>
  saveConversationMeta(metas: ConversationMeta[]): Promise<void>
  replaceConversationMessages(conversationId: string, messages: ChatMessage[]): Promise<void>
  onStream(handler: (e: StreamEvent) => void, conversationId?: string): Promise<UnlistenFn>
}

export const api: RuntimeApi = isTauriRuntime() ? tauriApi : webApi

export const sendChat = api.sendChat
export const cancelChat = api.cancelChat
export const abortTerminalCommand = api.abortTerminalCommand
export const approveToolCall = api.approveToolCall
export const getSettings = api.getSettings
export const updateSettings = api.updateSettings
export const updateAgentSettings = api.updateAgentSettings
export const updateUserSettings = api.updateUserSettings
export const updatePlatformSettings = api.updatePlatformSettings
export const setApiKey = api.setApiKey
export const clearApiKey = api.clearApiKey
export const testConnection = api.testConnection
export const listSkills = api.listSkills
export const reloadSkillMeta = api.reloadSkillMeta
export const importSkillZip = api.importSkillZip
export const probeExternalSkills = api.probeExternalSkills
export const importExternalSkills = api.importExternalSkills
export const dismissExternalSkillsPrompt = api.dismissExternalSkillsPrompt
export const listTools = api.listTools
export const listAgents = api.listAgents
export const getTaskBoardSnapshot = api.getTaskBoardSnapshot
export const previewComputerAnnotatedScreen = api.previewComputerAnnotatedScreen
export const previewComputerRoundScreen = api.previewComputerRoundScreen
export const previewChatMedia = api.previewChatMedia
export const previewMediaRef = api.previewMediaRef
export const saveChatAttachment = api.saveChatAttachment
export const checkMediaDeps = api.checkMediaDeps
export const revealInFinder = api.revealInFinder
export const listComputerMonitors = api.listComputerMonitors
export const setComputerCompactChrome = api.setComputerCompactChrome
export const reapplyWindowChrome = api.reapplyWindowChrome
export const setComputerConversationMonitor = api.setComputerConversationMonitor
export const confirmComputerMonitorPick = api.confirmComputerMonitorPick
export const cancelComputerMonitorPick = api.cancelComputerMonitorPick

const macosPermsOk = (): import('../types/macosPermissions').MacosComputerPermissionsStatus => ({
  screenRecording: true,
  screenRecordingPreflight: true,
  accessibility: true,
  appBundlePath: '',
  executablePath: '',
  bundleId: '',
  runningFromAppBundle: false
})

export const getMacosComputerPermissions = isTauriRuntime()
  ? tauriApi.getMacosComputerPermissions
  : async () => macosPermsOk()

export const registerMacosScreenRecordingAccess = isTauriRuntime()
  ? tauriApi.registerMacosScreenRecordingAccess
  : async () => {}

export const openMacosComputerPermissionSettings = isTauriRuntime()
  ? tauriApi.openMacosComputerPermissionSettings
  : async () => {}

export const beginMacosPermissionDragFlow = isTauriRuntime()
  ? tauriApi.beginMacosPermissionDragFlow
  : async () => {}

export const dismissMacosPermissionDragGuide = isTauriRuntime()
  ? tauriApi.dismissMacosPermissionDragGuide
  : async () => {}

export const loadConversations = api.loadConversations
export const saveConversations = api.saveConversations
export const saveConversationMeta = api.saveConversationMeta
export const replaceConversationMessages = api.replaceConversationMessages
export const onStream = api.onStream

export type PlatformSessionView = import('./tauri').PlatformSessionView
export const getPlatformSession = isTauriRuntime()
  ? tauriApi.getPlatformSession
  : webApi.getPlatformSession
export const openPlatformLogin = isTauriRuntime()
  ? tauriApi.openPlatformLogin
  : webApi.openPlatformLogin
export const cancelPlatformLogin = isTauriRuntime()
  ? tauriApi.cancelPlatformLogin
  : webApi.cancelPlatformLogin
export const refreshPlatformSession = isTauriRuntime()
  ? tauriApi.refreshPlatformSession
  : webApi.refreshPlatformSession
export const logoutPlatform = isTauriRuntime()
  ? tauriApi.logoutPlatform
  : webApi.logoutPlatform
export const loadPlatformSessionPersisted = isTauriRuntime()
  ? tauriApi.loadPlatformSessionPersisted
  : webApi.loadPlatformSessionFromKeyring
export const loadPlatformSessionFromKeyring = isTauriRuntime()
  ? tauriApi.loadPlatformSessionFromKeyring
  : webApi.loadPlatformSessionFromKeyring
