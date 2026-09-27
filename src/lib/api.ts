import type { UnlistenFn } from '@tauri-apps/api/event'
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
  fileName: string
  /** Desktop invoke / legacy: base64 wire. Prefer `file` on web or `sourcePath` on desktop. */
  contentBase64?: string
  /** Web: raw file for multipart/form-data upload. */
  file?: File
  /** Desktop: absolute local path — host copies into sandbox (no base64 IPC). */
  sourcePath?: string
}

export type AttachmentUploadProgress = {
  loaded: number
  total: number
  percent: number
}

export interface LocalFileAttachmentPayload {
  fileName: string
  mimeType: string
  sizeBytes: number
  contentBase64: string
}

/** Options for turn-windowed conversation message loads. */
export type LoadConversationMessagesPageOpts = {
  limitTurns?: number
  beforePosition?: number
  afterPosition?: number
  aroundMessageId?: string
  /** Default false: UI hydrate omits scoped sub-agent rows (stub + expand-on-demand). */
  includeScopedSubMessages?: boolean
}

export type LoadScopedSubMessagesOpts = {
  anchorMessageId: string
  /** Legacy; optional when `agentInstanceId` is set. */
  traceId?: string
  agentInstanceId?: string
}

/** Turn-windowed message page from SQLite (UI hydrate only). */
export type ConversationMessagePage = {
  messages: ChatMessage[]
  /** SQLite `position` of each message in `messages` (parallel array; wire-only). */
  positions?: number[]
  hasMoreOlder: boolean
  hasMoreNewer: boolean
  oldestPosition: number | null
  newestPosition: number | null
  messageCount: number
  /** Scoped rows by SpawnId; only when `includeScopedSubMessages` is true. */
  scoped?: Record<string, ChatMessage[]>
}

/** One row actually written by `append_conversation_messages` (wire-only). */
export type AppendedMessageRow = {
  messageId: string
  position: number
}

/** Default user-turn window for first paint / load-more / around. */
export const DEFAULT_MESSAGE_PAGE_TURNS = 8

export interface WorkspaceEntry {
  name: string
  path: string
  kind: 'file' | 'directory' | 'symlink'
  sizeBytes?: number
}

export interface WorkspaceFilePreview {
  path: string
  content?: string
  sizeBytes: number
  truncated: boolean
  binary: boolean
}

export interface GitChange {
  path: string
  status: string
  staged: boolean
  originalPath?: string
}

export interface GitDiff {
  path: string
  /** `unstaged` | `staged` | `untracked` */
  mode: string
  diffLines: Array<{
    type: 'unchanged' | 'del' | 'ins' | 'collapse'
    text: string
    hidden?: string[]
  }>
  diffStats: { adds?: number; dels?: number }
}

export interface TurnFileDiff {
  path: string
  baselineMissing: boolean
  created: boolean
  diffLines: Array<{
    type: 'unchanged' | 'del' | 'ins' | 'collapse'
    text: string
    hidden?: string[]
  }>
  diffStats: { adds?: number; dels?: number }
}

export interface TurnFileChangeEntry {
  path: string
  kind: string
  adds: number
  dels: number
}

export interface TurnFileChangesForTurn {
  turnId: string
  files: TurnFileChangeEntry[]
}

export type GitErrorCode = 'not_repository' | 'git_not_installed' | 'command_failed'

export interface GitErrorInfo {
  code: GitErrorCode
  message: string
}

export interface GitStatusResponse {
  changes: GitChange[]
  error?: GitErrorInfo
}

export interface SendChatPayload {
  conversationId: string
  messages: ChatMessage[]
  enabledSkillIds: string[]
  agentSkillOverrides?: Record<string, string[]>
  agentMode?: AgentMode
  /** Cumulative single-agent tool rounds before this send. */
  toolRoundsUsed?: number
  /** Cumulative Supervisor/sub-agent tool rounds before this send. */
  toolRoundsUsedSupervisor?: number
  /** Per-conversation workspace root for this run. */
  workspaceRoot?: string
  /** When true, backend uses session sandbox instead of inheriting another conversation's workspace. */
  workspaceInheritDisabled?: boolean
  /** Session lead worker for this run (`single` mode). */
  leadAgentId?: string
  /** Per-conversation performance tier override (Composer picker); unset = global default. */
  performanceMode?: PerformanceMode
}

export interface ConsoleSessionInfo {
  id: string
  workspaceRoot: string
  conversationId: string
  cwd: string
  label: string
}

export interface ConsoleSessionCreateInput {
  workspaceRoot: string
  conversationId: string
  cwd?: string
  cols: number
  rows: number
}

export interface RuntimeApi {
  sendChat(payload: SendChatPayload): Promise<string | void>
  cancelChat(
    conversationId: string,
    options?: { cancelBackgroundJobs?: boolean }
  ): Promise<void>
  /** Cancel background jobs by id; omit jobIds = all in conversation. Does not stop the lead turn. */
  cancelBackgroundJobs(conversationId: string, jobIds?: string[]): Promise<string[]>
  listWorkspaceDirectory(workspaceRoot: string, relativePath?: string): Promise<WorkspaceEntry[]>
  searchWorkspaceEntries(
    workspaceRoot: string,
    query: string,
    limit?: number
  ): Promise<WorkspaceEntry[]>
  readWorkspaceFile(workspaceRoot: string, relativePath: string): Promise<WorkspaceFilePreview>
  deleteWorkspacePath(workspaceRoot: string, relativePath: string): Promise<void>
  getWorkspaceGitStatus(workspaceRoot: string): Promise<GitStatusResponse>
  getWorkspaceGitDiff(
    workspaceRoot: string,
    relativePath: string,
    status?: string
  ): Promise<GitDiff>
  getTurnFileDiff(
    conversationId: string,
    turnId: string,
    workspaceRoot: string,
    path: string
  ): Promise<TurnFileDiff>
  listTurnFileChanges(
    conversationId: string,
    turnIds: string[]
  ): Promise<TurnFileChangesForTurn[]>
  saveTurnFileChanges(
    conversationId: string,
    turnId: string,
    files: TurnFileChangeEntry[]
  ): Promise<void>
  /** Stops only the in-flight `terminal` subprocess; the chat turn continues. */
  abortTerminalCommand(conversationId: string, toolCallId?: string): Promise<boolean>
  createConsoleSession(input: ConsoleSessionCreateInput): Promise<ConsoleSessionInfo>
  writeConsoleSession(sessionId: string, data: string): Promise<void>
  resizeConsoleSession(sessionId: string, cols: number, rows: number): Promise<void>
  closeConsoleSession(sessionId: string): Promise<boolean>
  approveToolCall(conversationId: string, toolCallId: string, approved: boolean): Promise<void>
  submitAskUser(toolCallId: string, selected: string[]): Promise<void>
  submitTerminalInput(requestId: string, text: string): Promise<void>
  dismissTerminalInput(requestId: string): Promise<void>
  getSettings(): Promise<EffectiveSettingsView>
  updateDebugSessionSettings(settings: DebugSessionSettings): Promise<DebugSessionSettings>
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
  listPlugins(): Promise<import('../types/plugin').PluginView[]>
  enablePlugin(pluginId: string): Promise<import('../types/plugin').PluginView>
  disablePlugin(pluginId: string): Promise<import('../types/plugin').PluginView>
  uninstallPlugin(pluginId: string): Promise<void>
  importPlugin(source: string): Promise<import('../types/plugin').ImportReport[]>
  /** 上传 zip 文件导入插件（web：/api/plugins/import-zip；desktop：读文件字节后调用）。 */
  importPluginZip(file: File): Promise<import('../types/plugin').ImportReport[]>
  discoverPlugins(dir: string): Promise<import('../types/plugin').DiscoveredPlugin[]>
  probeExternalPlugins(): Promise<import('../types/plugin').ExternalPluginsProbeResult>
  importExternalPlugin(sourceId: string): Promise<import('../types/plugin').ImportReport>
  listMcpServers(): Promise<import('../types/mcp').GlobalMcpView>
  saveMcpServers(servers: import('../types/mcp').McpServerDecl[]): Promise<import('../types/mcp').GlobalMcpView>
  reloadMcpServers(): Promise<import('../types/mcp').GlobalMcpView>
  restartMcpServer(): Promise<import('../types/mcp').GlobalMcpView>
  listTools(): Promise<ToolDef[]>
  listAgents(): Promise<AgentDef[]>
  getTaskBoardSnapshot(conversationId: string, taskId?: string): Promise<import('../types/chat').TaskBoardDocument>
  previewComputerAnnotatedScreen(conversationId: string): Promise<ComputerAnnotatedPreview>
  previewComputerRoundScreen(relPath: string): Promise<ComputerAnnotatedPreview>
  previewChatMedia(storageRelPath: string): Promise<ChatMediaPreview>
  getChatMediaLocalPath?(storageRelPath: string): Promise<string>
  previewMediaRef(mediaRef: string): Promise<ChatMediaPreview>
  saveChatAttachment(
    payload: SaveChatAttachmentPayload,
    onProgress?: (p: AttachmentUploadProgress) => void,
    options?: { signal?: AbortSignal }
  ): Promise<string>
  checkMediaDeps(): Promise<MediaDepsStatus>
  listComputerMonitors(): Promise<ComputerMonitor[]>
  setComputerCompactChrome(compact: boolean): Promise<void>
  beginComputerCompactWindow(): Promise<void>
  restoreComputerCompactWindowNative(): Promise<void>
  placeComputerCompactWindow(width: number, height: number, margin: number): Promise<void>
  reapplyWindowChrome(): Promise<void>
  setComputerConversationMonitor(conversationId: string, monitorId: string | null): Promise<void>
  confirmComputerMonitorPick(conversationId: string): Promise<void>
  cancelComputerMonitorPick(conversationId: string): Promise<void>
  getMacosComputerPermissions?(): Promise<import('../types/macosPermissions').MacosComputerPermissionsStatus>
  openMacosComputerPermissionSettings?(kind: import('./tauri').MacosPermissionDragKind): Promise<void>
  beginMacosPermissionDragFlow?(kind: import('./tauri').MacosPermissionDragKind): Promise<void>
  dismissMacosPermissionDragGuide?(): Promise<void>

  loadConversations(): Promise<Conversation[]>
  /** Cursor-paginated meta-only list (no messages). Sort: updatedAt DESC, id DESC. */
  loadConversationMetas(cursor: ConversationCursor | null, limit?: number): Promise<ConversationMetaPage>
  /** Single meta by id; null when missing or outside the caller's list scope. */
  loadConversationMeta(conversationId: string): Promise<ConversationMeta | null>
  loadProjects(cursor: ProjectCursor | null, limit?: number): Promise<ProjectPage>
  loadSidebarProjects(): Promise<Project[]>
  loadProject(projectId: string): Promise<Project | null>
  loadProjectConversationMetas(projectId: string, cursor: ConversationCursor | null, limit?: number): Promise<ConversationMetaPage>
  createProject(name: string, workspaceRoot: string): Promise<ProjectCreationResult>
  createDirectory(parentPath: string, name: string): Promise<string>
  updateProject(id: string, patch: Partial<Pick<Project, 'name' | 'workspaceRoot' | 'isPinned' | 'isArchived'>>): Promise<Project>
  deleteProject(id: string): Promise<void>
  searchConversations(query: string, limit?: number): Promise<ConversationSearchHit[]>
  listConversationSearchMatches(
    conversationId: string,
    query: string
  ): Promise<import('../types/chat').ConversationSearchMatch[]>
  listConversationOutline(conversationId: string): Promise<import('../types/chat').ConversationOutlineItem[]>
  setMessageMilestone(conversationId: string, messageId: string, milestone: boolean): Promise<void>
  loadConversationMessages(conversationId: string): Promise<ChatMessage[]>
  /** One transcript row. Null when the id is not on disk yet. */
  loadConversationMessage(conversationId: string, messageId: string): Promise<ChatMessage | null>
  /** Turn-windowed hydrate (tail / before / around). Prefer this over full load for UI. */
  loadConversationMessagesPage(
    conversationId: string,
    opts?: LoadConversationMessagesPageOpts
  ): Promise<ConversationMessagePage>
  loadScopedSubMessagesForTrace(
    conversationId: string,
    opts: LoadScopedSubMessagesOpts
  ): Promise<ChatMessage[]>
  revealInFinder(path: string): Promise<void>
  openPathWithDefaultApp(path: string): Promise<void>
  openChatMedia(storageRelPath: string): Promise<void>
  readLocalFileForAttachment(path: string): Promise<LocalFileAttachmentPayload>
  saveConversationMeta(metas: ConversationMeta[]): Promise<void>
  deleteConversation(conversationId: string): Promise<void>
  appendConversationMessages(
    conversationId: string,
    messages: ChatMessage[]
  ): Promise<AppendedMessageRow[]>
  onStream(
    handler: (e: StreamEvent) => void,
    conversationId?: string,
    onGap?: (reason: string) => void
  ): Promise<UnlistenFn>

  // ---- Phase 5/6: automation (cron jobs + webhook token) ----
  listCronJobs(): Promise<import('../types/automation').CronJob[]>
  listCronDeliveryTargets(): Promise<import('../types/automation').CronDeliveryTarget[]>
  createCronJob(input: import('../types/automation').CreateCronJobInput): Promise<import('../types/automation').CronJob>
  updateCronJob(jobId: string, input: import('../types/automation').UpdateCronJobInput): Promise<import('../types/automation').CronJob>
  deleteCronJob(jobId: string): Promise<boolean>
  getWebhookConfig(): Promise<import('../types/automation').WebhookConfig>
  setWebhookSourceToken(
    src: string,
    token: string,
    authHeaderName?: string | null,
    sessionMode?: import('../types/automation').WebhookSessionMode
  ): Promise<import('../types/automation').WebhookConfig>
  patchWebhookSource(
    src: string,
    patch: { sessionMode?: import('../types/automation').WebhookSessionMode }
  ): Promise<import('../types/automation').WebhookConfig>
  clearWebhookSourceToken(src: string): Promise<boolean>
  clearWebhookLegacyToken(): Promise<boolean>
  revealWebhookSourceToken(src: string): Promise<import('../types/automation').WebhookTokenReveal>
  getDispatcherQueueSnapshot(): Promise<import('../types/automation').RunQueueSnapshot>
}

export const api: RuntimeApi = isTauriRuntime() ? tauriApi : webApi

export const sendChat = api.sendChat
export const cancelChat = api.cancelChat
export const cancelBackgroundJobs = api.cancelBackgroundJobs
export const listWorkspaceDirectory = api.listWorkspaceDirectory
export const searchWorkspaceEntries = api.searchWorkspaceEntries
export const readWorkspaceFile = api.readWorkspaceFile
export const deleteWorkspacePath = api.deleteWorkspacePath
export const getWorkspaceGitStatus = api.getWorkspaceGitStatus
export const getWorkspaceGitDiff = api.getWorkspaceGitDiff
export const getTurnFileDiff = api.getTurnFileDiff
export const listTurnFileChanges = api.listTurnFileChanges
export const saveTurnFileChanges = api.saveTurnFileChanges
export const abortTerminalCommand = api.abortTerminalCommand
export const createConsoleSession = api.createConsoleSession
export const writeConsoleSession = api.writeConsoleSession
export const resizeConsoleSession = api.resizeConsoleSession
export const closeConsoleSession = api.closeConsoleSession
export const approveToolCall = api.approveToolCall
export const submitAskUser = api.submitAskUser
export const submitTerminalInput = api.submitTerminalInput
export const dismissTerminalInput = api.dismissTerminalInput
export const getSettings = api.getSettings
export const updateDebugSessionSettings = api.updateDebugSessionSettings
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
export const listPlugins = api.listPlugins
export const enablePlugin = api.enablePlugin
export const disablePlugin = api.disablePlugin
export const uninstallPlugin = api.uninstallPlugin
export const importPlugin = api.importPlugin
export const importPluginZip = api.importPluginZip
export const discoverPlugins = api.discoverPlugins
export const probeExternalPlugins = api.probeExternalPlugins
export const importExternalPlugin = api.importExternalPlugin
export const listMcpServers = api.listMcpServers
export const saveMcpServers = api.saveMcpServers
export const reloadMcpServers = api.reloadMcpServers
export const restartMcpServer = api.restartMcpServer
export const listTools = api.listTools
export const listAgents = api.listAgents
export const getTaskBoardSnapshot = api.getTaskBoardSnapshot
export const previewComputerAnnotatedScreen = api.previewComputerAnnotatedScreen
export const previewComputerRoundScreen = api.previewComputerRoundScreen
export const previewChatMedia = api.previewChatMedia
export const getChatMediaLocalPath = api.getChatMediaLocalPath
export const previewMediaRef = api.previewMediaRef
export const saveChatAttachment = api.saveChatAttachment
export const checkMediaDeps = api.checkMediaDeps
export const revealInFinder = api.revealInFinder
export const openPathWithDefaultApp = api.openPathWithDefaultApp
export const openChatMedia = api.openChatMedia
export const downloadChatMedia = isTauriRuntime()
  ? async (_path: string, _fileName?: string) => {
      throw new Error('downloadChatMedia is not supported in desktop runtime')
    }
  : webApi.downloadChatMedia
export const downloadChatMediaRef = isTauriRuntime()
  ? async (_ref: string, _fileName?: string) => {
      throw new Error('downloadChatMediaRef is not supported in desktop runtime')
    }
  : webApi.downloadChatMediaRef
export const chatMediaDownloadUrl = isTauriRuntime()
  ? (_path: string) => {
      throw new Error('chatMediaDownloadUrl is not supported in desktop runtime')
    }
  : webApi.chatMediaDownloadUrl
export const chatMediaRefDownloadUrl = isTauriRuntime()
  ? (_ref: string) => {
      throw new Error('chatMediaRefDownloadUrl is not supported in desktop runtime')
    }
  : webApi.chatMediaRefDownloadUrl
export const readLocalFileForAttachment = api.readLocalFileForAttachment
export const listComputerMonitors = api.listComputerMonitors
export const setComputerCompactChrome = api.setComputerCompactChrome
export const beginComputerCompactWindow = api.beginComputerCompactWindow
export const restoreComputerCompactWindowNative = api.restoreComputerCompactWindowNative
export const placeComputerCompactWindow = api.placeComputerCompactWindow
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
export const loadConversationMetas = api.loadConversationMetas
export const loadConversationMeta = api.loadConversationMeta
export const loadProjects = api.loadProjects
export const loadSidebarProjects = api.loadSidebarProjects
export const loadProject = api.loadProject
export const loadProjectConversationMetas = api.loadProjectConversationMetas
export const createProject = api.createProject
export const createDirectory = api.createDirectory
export const updateProject = api.updateProject
export const deleteProject = api.deleteProject
export const searchConversations = api.searchConversations
export const listConversationSearchMatches = api.listConversationSearchMatches
export const listConversationOutline = api.listConversationOutline
export const setMessageMilestone = api.setMessageMilestone
export const loadConversationMessages = api.loadConversationMessages
export const loadConversationMessagesPage = api.loadConversationMessagesPage
export const loadConversationMessage = api.loadConversationMessage
export const loadScopedSubMessagesForTrace = api.loadScopedSubMessagesForTrace
export const saveConversationMeta = api.saveConversationMeta
export const deleteConversation = api.deleteConversation
export const appendConversationMessages = api.appendConversationMessages
export const onStream = api.onStream
export const waitForChatStreamReady = isTauriRuntime()
  ? tauriApi.waitForChatStreamReady
  : webApi.waitForChatStreamReady

export type PlatformSessionView = import('./tauri').PlatformSessionView
export type AuthMode = import('./web').AuthMode
export type LocalCaptcha = import('./web').LocalCaptcha

export const getAuthMode = isTauriRuntime()
  ? (async (): Promise<AuthMode> => 'platform')
  : webApi.getAuthMode
export const fetchLocalCaptcha = isTauriRuntime()
  ? (async (): Promise<LocalCaptcha> => {
      throw new Error('local captcha is only available on pointer-server web')
    })
  : webApi.fetchLocalCaptcha
export const localLogin = isTauriRuntime()
  ? (async (): Promise<PlatformSessionView> => {
      throw new Error('local login is only available on pointer-server web')
    })
  : webApi.localLogin

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

export type TokenUsageListItem = import('./tauri').TokenUsageListItem
export type TokenUsageListResult = import('./tauri').TokenUsageListResult
export const listTokenUsage = isTauriRuntime()
  ? tauriApi.listTokenUsage
  : webApi.listTokenUsage

// ---- Phase 5/6: automation (cron jobs + webhook token) ----
export const listCronJobs = api.listCronJobs
export const listCronDeliveryTargets = api.listCronDeliveryTargets
export const createCronJob = api.createCronJob
export const updateCronJob = api.updateCronJob
export const deleteCronJob = api.deleteCronJob
export const getWebhookConfig = api.getWebhookConfig
export const setWebhookSourceToken = api.setWebhookSourceToken
export const patchWebhookSource = api.patchWebhookSource
export const clearWebhookSourceToken = api.clearWebhookSourceToken
export const clearWebhookLegacyToken = api.clearWebhookLegacyToken
export const revealWebhookSourceToken = api.revealWebhookSourceToken
export const getDispatcherQueueSnapshot = api.getDispatcherQueueSnapshot
