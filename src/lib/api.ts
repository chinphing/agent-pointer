import type { UnlistenFn } from '@tauri-apps/api/event'
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

import * as tauriApi from './tauri'
import * as webApi from './web'
import { isTauriRuntime } from './runtime'

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentMode?: AgentMode
  /** Cumulative single-agent tool rounds before this send. */
  toolRoundsUsed?: number
  /** Cumulative Supervisor/sub-agent tool rounds before this send. */
  toolRoundsUsedSupervisor?: number
}

export interface RuntimeApi {
  sendChat(payload: SendChatPayload): Promise<string | void>
  cancelChat(conversationId: string): Promise<void>
  /** Stops only the in-flight `terminal` subprocess; the chat turn continues. */
  abortTerminalCommand(conversationId: string): Promise<boolean>
  approveToolCall(conversationId: string, toolCallId: string, approved: boolean): Promise<void>
  getSettings(): Promise<ModelSettings>
  updateSettings(settings: ModelSettings): Promise<ModelSettings>
  setApiKey(key: string): Promise<void>
  clearApiKey(): Promise<void>
  testConnection(): Promise<{ ok: boolean; latencyMs: number; message: string }>
  listSkills(): Promise<SkillDef[]>
  importSkillZip(file: File): Promise<SkillImportResult>
  listTools(): Promise<ToolDef[]>
  listAgents(): Promise<AgentDef[]>
  getTaskBoardSnapshot(conversationId: string, taskId?: string): Promise<import('../types/chat').TaskBoardDocument>
  previewComputerAnnotatedScreen(conversationId: string): Promise<ComputerAnnotatedPreview>
  previewComputerRoundScreen(relPath: string): Promise<ComputerAnnotatedPreview>
  listComputerMonitors(): Promise<ComputerMonitor[]>
  setComputerConversationMonitor(conversationId: string, monitorId: string | null): Promise<void>

  loadConversations(): Promise<Conversation[]>
  saveConversations(conversations: Conversation[]): Promise<void>
  onStream(handler: (e: StreamEvent) => void, conversationId?: string): Promise<UnlistenFn>
}

export const api: RuntimeApi = isTauriRuntime() ? tauriApi : webApi

export const sendChat = api.sendChat
export const cancelChat = api.cancelChat
export const abortTerminalCommand = api.abortTerminalCommand
export const approveToolCall = api.approveToolCall
export const getSettings = api.getSettings
export const updateSettings = api.updateSettings
export const setApiKey = api.setApiKey
export const clearApiKey = api.clearApiKey
export const testConnection = api.testConnection
export const listSkills = api.listSkills
export const importSkillZip = api.importSkillZip
export const listTools = api.listTools
export const listAgents = api.listAgents
export const getTaskBoardSnapshot = api.getTaskBoardSnapshot
export const previewComputerAnnotatedScreen = api.previewComputerAnnotatedScreen
export const previewComputerRoundScreen = api.previewComputerRoundScreen
export const listComputerMonitors = api.listComputerMonitors
export const setComputerConversationMonitor = api.setComputerConversationMonitor

export const loadConversations = api.loadConversations
export const saveConversations = api.saveConversations
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
export const loadPlatformSessionFromKeyring = isTauriRuntime()
  ? tauriApi.loadPlatformSessionFromKeyring
  : webApi.loadPlatformSessionFromKeyring
