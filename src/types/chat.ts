export type Role = 'system' | 'user' | 'assistant' | 'tool'

export interface WebSearchSourceEntry {
  index: number
  title: string
  url: string
  siteName?: string
}

export type MessageStatus = 'pending' | 'streaming' | 'done' | 'error' | 'cancelled'

export interface ToolCall {
  id: string
  name: string
  arguments: string
  status: 'pending' | 'pending_approval' | 'running' | 'success' | 'failed' | 'rejected'
  result?: string
  error?: string
  durationMs?: number
  riskLevel?: 'low' | 'medium' | 'high'
  terminalOutput?: string
  /** Streaming search answer text (web_search tool). */
  webSearchOutput?: string
  /** Sources ready from first SSE chunk (web_search tool). */
  webSearchSources?: WebSearchSourceEntry[]
  /** UI-only Chinese label from backend (not sent to the LLM). */
  displayLabel?: string
  /** UI-only short parameter summary from backend. */
  displaySummary?: string
  /** Terminal tool is waiting for user input in a modal. */
  waitingForInput?: boolean
}

export const DEFAULT_LEAD_AGENT_ID = 'general'

/** Bundled skills enabled for new users when lead agent is `general`. Keep in sync with pointer-core `DEFAULT_ENABLED_SKILL_IDS`. */
/** @deprecated Runtime uses agentSkillOverrides + agent defaultSkillIds. Kept for docs/sync. */
export const DEFAULT_ENABLED_SKILL_IDS = [
  'find-skills',
  'dev-env-setup',
  'skill-manager',
  'pointer-manager',
  'docx',
  'xlsx',
  'pptx',
  'pdf',
  'agent-browser'
] as const

export type AgentMode = 'single' | 'supervisor'

export type ThemePreference = 'light' | 'dark' | 'system'

/** Computer agent vision tier (matches backend `ComputerTier`). */
export type ComputerInitialTier = 'primary' | 'intermediate' | 'advanced'

export const COMPUTER_INITIAL_TIER_OPTIONS: { value: ComputerInitialTier; label: string }[] = [
  { value: 'primary', label: '快速' },
  { value: 'intermediate', label: '标准' },
  { value: 'advanced', label: '专家' }
]

/** Per-agent chat UI visibility (from AGENT.md `ui` block). */
export interface AgentUiConfig {
  showInComposer?: boolean
  /** Debug: show sidecar tool calls (`verify:*`, `task_board_patch`, etc.). */
  showSidecarToolCalls?: boolean
  /** Debug: show non-sidecar tool calls (real action tools). */
  showNonSidecarToolCalls?: boolean
  showReasoning?: boolean
  showSubAgentTrace?: boolean
  showToolCalls?: boolean
  /** Debug: show full tool result JSON in expanded tool cards. */
  showToolCallResults?: boolean
  hideToolNames?: string[]
  showWorkspacePicker?: boolean
  showComputerMonitorPicker?: boolean
  showTaskBoardPanel?: boolean
  /** Debug: show child task boards under parent board panel. */
  showTaskBoardChildren?: boolean
  /** When true, user may pick this agent in the chat composer (not settings smart mode). */
  userSelectable?: boolean
  /** Label shown in the chat composer agent picker (UI only). */
  composerLabel?: string
  avatar?: string
}

export interface SubAgentToolStats {
  /** file_grep / file_glob / file_list / session_search / memory */
  searchCount: number
  /** file_read */
  readCount: number
  /** file_write / file_edit */
  writeCount?: number
  /** terminal */
  terminalCount?: number
  /** web_search */
  webSearchCount?: number
  /** skill_read / skill_import */
  skillCount?: number
  /** media_understand / image_generate / video_generate */
  mediaCount?: number
  /** Computer: mouse_* */
  mouseCount?: number
  /** Computer: input_* */
  inputCount?: number
  /** Computer: hotkey, wait, clipboard, captcha, etc. */
  otherCount?: number
}

export interface SubAgentSessionUi {
  thoughts?: string
  headline?: string
  toolNamePreview?: string
  responseTextDraft?: string
  reasoning?: string
  rawContent?: string
  contentStreaming?: boolean
  toolCalls?: ToolCall[]
  stats: SubAgentToolStats
  summaryLine?: string
  collapsed: boolean
  userExpanded: boolean
}

/** Delegated computer task: automate Pointer UI vs external desktop apps. */
export type ComputerOperationTarget = 'self' | 'external'

export interface AgentTrace {
  id: string
  name: string
  role: string
  status: string
  detail?: string
  content?: string
  /** 0 = 顶格（主编排），1 = 委托子 Agent；缺省时 UI 对首条顶格、其余一级缩进 */
  depth?: number
  /** Legacy nested UI session; superseded by scoped child `ChatMessage` rows. Read-only for old data. */
  session?: SubAgentSessionUi
  /** Collapsed summary header (persisted index UI). */
  collapsed?: boolean
  userExpanded?: boolean
  /** Runtime child invocation UUID used to isolate reused task traces. */
  agentInstanceId?: string
  /** `run_subagent` → computer: task goal targets Pointer itself (`self`) or other apps (`external`). */
  computerTarget?: ComputerOperationTarget
  /** Parent assistant `run_subagent` tool-call id; nest the frame under that tool row. */
  parentToolCallId?: string
  /** The `ChatMessage.id` that owns this trace's scoped child messages (for nested sub-agents this differs from the lead anchor). */
  anchorMessageId?: string
}

export type AgentProfile =
  | 'general'
  | 'supervisor'
  | 'planner'
  | 'coder'
  | 'reviewer'
  | 'writer'
  | 'analyst'
  | 'tool_user'
  | 'computer'
  | 'explore'
  | { custom: string }

export interface AccessPolicy {
  allowTools: string[]
  denyTools: string[]
  /** Legacy; ignored at runtime. Skill boundary is defaultSkillIds + override. */
  allowSkills: string[]
  /** Legacy; ignored at runtime. Skill boundary is defaultSkillIds + override. */
  denySkills: string[]
}

export interface AgentDef {
  id: string
  name: string
  description: string
  role: string
  profile: AgentProfile
  defaultSkillIds: string[]
  accessPolicy: AccessPolicy
  builtin: boolean
  enabled: boolean
  toolNames: string[]
  /** Worker ids this lead may pass to `run_subagent` (AGENT.md frontmatter). */
  allowAgents?: string[]
  source?: string
  resourceFiles: string[]
  ui?: AgentUiConfig
}

export interface MessageUiBindings {
  taskBoardAnchor?: boolean
}

export type ExcludedReason =
  | 'context_compression'
  | 'task_board_trim'
  | 'planner_ui_shell'

export interface MessageContextState {
  included: boolean
  excludedReason?: ExcludedReason
}

export type MediaAttachmentKind = 'image' | 'document' | 'audio' | 'video' | 'file'

/** User message attachment metadata (wire may include contentBase64). */
export interface MediaAttachment {
  id: string
  kind: MediaAttachmentKind
  mimeType: string
  fileName: string
  sizeBytes: number
  storageRelPath?: string
  /** Absolute local path for assistant reply `MEDIA:` preview in App UI. */
  localAbsPath?: string
  /** Wire-only; stripped before disk persist. */
  contentBase64?: string
  derivedText?: string
  /** OSS HTTPS URL (video attachments uploaded at compose time). */
  remoteUrl?: string
  ossObjectKey?: string
  /** UI-only; stripped before disk persist. */
  previewUrl?: string
}

/** User send held in the outbound FIFO until the current session turn finishes. */
export interface OutboundQueueItem {
  id: string
  content: string
  attachments?: MediaAttachment[]
  createdAt: number
}

export type VideoUploadState = 'pending' | 'compressing' | 'uploading' | 'done' | 'error'

export interface MediaModelOverrides {
  image?: AgentModelRef
  audio?: AgentModelRef
  video?: AgentModelRef
  /** Image generation tool (`image_generate`). */
  imageGeneration?: AgentModelRef
  /** Video generation tool (`video_generate`). */
  videoGeneration?: AgentModelRef
}

export type FfmpegToolStatus = 'ready' | 'not_found' | 'partial' | 'not_executable'

export interface MediaDepsStatus {
  /** True only when ffmpeg and ffprobe are found and respond to `-version`. */
  ffmpegAvailable: boolean
  status: FfmpegToolStatus
  ffprobeAvailable: boolean
  ffmpegPath?: string
  ffprobePath?: string
  detail?: string
}

/** Pending composer attachment (metadata + optional preview URL). */
export interface ComposerAttachment extends MediaAttachment {
  previewUrl?: string
  /** Absolute source path while OSS upload runs (desktop file picker). */
  localSourcePath?: string
  /** Persist/upload progress (images/files and video OSS). */
  uploadState?: VideoUploadState
  uploadProgress?: number
  uploadError?: string
}

/** In-memory composer draft (text + pending attachments); not persisted to disk. */
export interface ComposerDraft {
  text: string
  attachments: ComposerAttachment[]
}

export interface ChatMessage {
  id: string
  role: Role
  content: string
  status: MessageStatus
  createdAt: number
  toolCalls?: ToolCall[]
  toolCallId?: string
  errorMessage?: string
  /** API `reasoning_content`（深度求索等）；仅「原始输出」调试用，主气泡不展示。 */
  reasoning?: string
  /** JSON `thoughts` from the model response object (last turn). */
  thoughts?: string
  /** JSON `headline` from the model response object. */
  headline?: string
  /** Streaming preview of `tool_name` before the JSON object is complete. */
  toolNamePreview?: string
  /** Streaming `response` tool `tool_args.text` while JSON is still incomplete. */
  responseTextDraft?: string
  /** True while the current LLM round is actively streaming (false between tool rounds). */
  contentStreaming?: boolean
  rawContent?: string
  /** Concatenated raw outputs from tool invocations in this assistant turn. */
  toolRawOutput?: string
  agentId?: string
  /** Runtime agent launch UUID (one per lead / sub-agent invocation). */
  agentInstanceId?: string
  agentName?: string
  agentTrace?: AgentTrace[]
  /** Supervisor plan checklist (stream `supervisor_plan`). */
  supervisorPlanTasks?: SupervisorPlanTask[]
  /** Annotated PNG path under app `computer-captures/` (lazy load on preview); persisted when the stream emits it. */
  computerRoundScreenRelPath?: string
  /** UI mount hints (e.g. TaskBoard anchor). */
  uiBindings?: MessageUiBindings
  /** Whether this message participates in LLM context. */
  contextState?: MessageContextState
  attachments?: MediaAttachment[]
  /** Parent lead assistant message id (scoped sub-agent transcript rows). */
  anchorMessageId?: string
  /** Stable sub-task trace id; self-forks include a unique instance segment. */
  traceId?: string
  taskId?: string
  spawnDepth?: number
}

export interface Conversation {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  /** Pinned conversations stay above others in sidebar lists. */
  isPinned?: boolean
  messages: ChatMessage[]
  skillIds: string[]
  /** Cumulative tool rounds for single-agent replies (cap in settings). */
  toolRoundsUsed?: number
  /** Cumulative tool rounds for Supervisor / sub-agents (separate cap pool). */
  toolRoundsUsedSupervisor?: number
  /** Selected desktop monitor for Computer agent; empty = auto (monitor under cursor). */
  computerMonitorId?: string
  /** Persisted project that owns this conversation after first-send binding. */
  projectId?: string
  /** Composer-only project choice awaiting first-send binding. */
  pendingProjectId?: string
  /** Per-conversation workspace for coder/file tools (set in composer). */
  workspaceRoot?: string
  /** User explicitly picked workspaceRoot in composer (not auto sandbox). */
  workspaceUserSet?: boolean
  /** User cleared workspace; prefer session sandbox over inherit. */
  workspaceInheritDisabled?: boolean
  /** Per-conversation lead worker when agentMode is single. */
  leadAgentId?: string
  /** Per-conversation orchestration mode. */
  agentMode?: AgentMode
  /** DB-backed message count (populated on meta-only list load; not present on legacy full-load). */
  messageCount?: number
}

/** Session shell fields for meta-only persistence (P1). */
export type ConversationMetaBase = Pick<
  Conversation,
  | 'id'
  | 'title'
  | 'createdAt'
  | 'updatedAt'
  | 'isPinned'
  | 'skillIds'
  | 'toolRoundsUsed'
  | 'toolRoundsUsedSupervisor'
  | 'computerMonitorId'
  | 'projectId'
  | 'workspaceRoot'
  | 'workspaceUserSet'
  | 'workspaceInheritDisabled'
  | 'leadAgentId'
  | 'agentMode'
>

/** Result of creating a project; duplicate workspace roots reuse the persisted project. */
export interface ProjectCreationResult {
  project: Project
  reusedExisting: boolean
}

/** Conversation shell + DB-backed summary fields (no messages). */
export interface ConversationMeta extends ConversationMetaBase {
  /** Persisted message count (DB-backed; 0 when derived from in-memory Conversation). */
  messageCount?: number
  /** Short preview of latest messages (DB-backed; empty when derived from in-memory Conversation). */
  preview?: string
}

/** A persisted workspace project that owns conversations. */
export interface Project {
  id: string
  name: string
  workspaceRoot: string
  isDefault: boolean
  isPinned: boolean
  isArchived: boolean
  createdAt: number
  updatedAt: number
  /** Newest owned conversation activity, or createdAt when the project is empty. */
  lastActivityAt: number
  /** Owner (`SSO sub` / OAuth id / local-admin). Empty = legacy/anonymous. */
  sessionUserId?: string
}

export interface ProjectPage {
  items: Project[]
  nextCursor: ProjectCursor | null
}

/** Cursor for project paging (sort: pinned DESC, lastActivityAt DESC, id DESC). */
export interface ProjectCursor {
  lastActivityAt: number
  id: string
}

/** Cursor for paginated conversation-meta list (sort: pinned DESC, updatedAt DESC, id DESC). */
export interface ConversationCursor {
  updatedAt: number
  id: string
}

/** One page of conversation metas from the cursor-paginated list API. */
export interface ConversationMetaPage {
  items: ConversationMeta[]
  nextCursor: ConversationCursor | null
}

/** Sidebar FTS search hit (message content and/or title/preview match). */
export interface ConversationSearchHit {
  id: string
  title: string
  updatedAt: number
  snippet?: string
  /** Matched message id when hit is from message body; omit for title-only matches. */
  messageId?: string
  messageCount?: number
  projectId?: string
  preview?: string
}

/** Per-model API/runtime overrides; unset fields inherit from the parent provider. */
export interface ModelRuntimeOverrides {
  reasoningInMessages?: boolean
  temperature?: number
  maxTokens?: number
  /** Qwen: deep thinking (`enable_thinking` on wire). */
  enableThinking?: boolean
  /** Qwen: thinking token budget when `enableThinking` is true. */
  thinkingBudget?: number
  /** DeepSeek: `reasoning_effort` — `high` | `max`. */
  reasoningEffort?: 'high' | 'max'
  /** Whether the model accepts vision / image understanding input. */
  supportsVision?: boolean
  /** Whether the model can generate images (`image_generate`). */
  canGenerateImage?: boolean
  /** Whether the model can generate videos (`video_generate`). */
  canGenerateVideo?: boolean
}

/** User-facing performance tier for chat / media understanding agents. */
export type PerformanceMode = 'fast' | 'standard' | 'expert'

export type PerformanceModeKey = PerformanceMode

export const PERFORMANCE_MODE_OPTIONS: { value: PerformanceMode; label: string }[] = [
  { value: 'fast', label: '快速' },
  { value: 'standard', label: '标准' },
  { value: 'expert', label: '专家' }
]

export interface MediaUnderstandingModes {
  image?: PerformanceMode
  audio?: PerformanceMode
  video?: PerformanceMode
}

/** agentId → user-selected performance mode (general / coder). */
export type AgentPerformanceModes = Partial<Record<string, PerformanceMode>>

/** Debug: agentId → mode → LLM profile (like computer tier models). */
export type AgentModeLlmMap = Partial<
  Record<string, Partial<Record<PerformanceModeKey, ComputerTierLlmConfig>>>
>

/** Debug: media kind → mode → LLM profile. */
export type MediaModeLlmMap = Partial<
  Record<'image' | 'audio' | 'video', Partial<Record<PerformanceModeKey, ComputerTierLlmConfig>>>
>

export interface ProviderConfig {
  id: string
  name: string
  baseUrl: string
  apiKey: string
  models: string[]
  /** Default for all models under this provider when `modelConfigs[model]` has no override. */
  reasoningInMessages?: boolean
  /** Default creativity for models without a per-model override. */
  temperature?: number
  /** Default max output tokens for models without a per-model override. */
  maxTokens?: number
  modelConfigs?: Record<string, ModelRuntimeOverrides>
  /** Qwen: deep thinking (`enable_thinking` on wire). */
  enableThinking?: boolean
  /** Qwen: thinking token budget when `enableThinking` is true. */
  thinkingBudget?: number
  /** DeepSeek: `reasoning_effort` — `high` | `max`. */
  reasoningEffort?: 'high' | 'max'
}

export interface AgentModelRef {
  providerId: string
  model: string
}

export interface UserSettings {
  theme?: ThemePreference
  userNickname?: string
  /**
   * Legacy globally enabled skill ids. Not used at runtime; migrated into
   * `agentSkillOverrides.general` on startup when that override is absent.
   */
  enabledSkillIds?: string[]
  /** Per-agent enabled skill ids — sole runtime enablement source. */
  agentSkillOverrides?: Record<string, string[]>
  /** Shrink app window to dock bar while computer agent is executing (default true). */
  computerAutoCompact?: boolean
  /** Play a short chime when a chat turn finishes (default true). */
  playSoundOnFinish?: boolean
  /** Global coding preferences injected as [USER RULES] in agent system prompt. */
  userCodingRules?: string
  /** Aliyun OSS for large video understanding (HTTP video_url to DashScope). */
  mediaOss?: MediaOssConfig
}

/** Aliyun OSS — large video temp upload for native DashScope `video_url`. */
export interface MediaOssConfig {
  enabled?: boolean
  bucket?: string
  /** Region id, e.g. `cn-hangzhou`. */
  region?: string
  /** Optional endpoint, e.g. `https://oss-cn-hangzhou.aliyuncs.com`. */
  endpoint?: string
  accessKeyId?: string
  accessKeySecret?: string
  keyPrefix?: string
  presignExpiresSec?: number
  deleteAfterUse?: boolean
}

export interface ComputerTierLlmConfig {
  providerId: string
  model: string
  enableThinking?: boolean
  thinkingBudget?: number
}

/** Per-phase model ids and thinking budgets for host verify pipeline (debug). */
export interface ComputerPipelineLlmSettings {
  decision?: string
  position?: string
  verify?: string
  decisionProviderId?: string
  positionProviderId?: string
  verifyProviderId?: string
  positionThinkingBudget?: number
  verifyThinkingBudget?: number
}

export type ComputerTierKey = 'primary' | 'intermediate' | 'advanced'

/** Process-local debug model configuration. Never persist this object. */
export interface DebugSessionSettings {
  providers: ProviderConfig[]
  activeProviderId: string
  model: string
  temperature: number
  maxTokens: number
  computerTierLlm: Partial<Record<ComputerTierKey, ComputerTierLlmConfig>>
  computerPipelineLlm: ComputerPipelineLlmSettings
  agentModeLlm: AgentModeLlmMap
  mediaModeLlm: MediaModeLlmMap
}

/** Platform/runtime fields (in-memory; admin-editable in desktop app). */
export interface PlatformSettings {
  providers: ProviderConfig[]
  activeProviderId: string
  model: string
  temperature: number
  maxTokens: number
  toolApprovalMode: 'auto' | 'manual'
  agentMode: AgentMode
  workspaceRoot: string
  leadAgentId: string
  contextCompressionEnabled: boolean
  contextBudgetTokens: number
  contextKeepRecentUserTurns: number
  contextSummaryMaxTokens: number
  maxToolRounds: number
  maxSubAgentToolRounds?: number
  maxSubAgentSpawnDepth?: number
  rawContentViewEnabled: boolean
  /** Write each LLM request payload to app data `logs/llm_prompts/` (debug) */
  debugDumpLlmPrompts?: boolean
  /**
   * Debug: env KEY→VALUE overlays for `terminal` child processes.
   * Overrides process / `.env` / session values; `PATH` is prepended like `.env`.
   * Applied only while debug menus are enabled.
   */
  terminalEnvOverrides?: Record<string, string>
  /** Settings dialog debug sections toggle (independent of rawContentView / dump prompts) */
  debugMenusEnabled?: boolean
  /** Debug: show child task boards under parent board panel. */
  taskBoardShowChildBoards?: boolean
  agentDefaultModels: Record<string, AgentModelRef>
  agentTaskBoardHistoryTrim?: Record<string, boolean>
  computerHumanLike?: boolean
  computerInitialTier?: ComputerInitialTier
  /** Auto-select primary monitor and follow launch_app window; when false, manual spatial picker */
  computerAutoSwitchMonitor?: boolean
  /** Show annotated screenshot preview on Computer Use assistant messages */
  computerAnnotatedScreenViewEnabled?: boolean
  /** Pixel offset added to final slider CAPTCHA drag point */
  captchaSliderOffsetPx?: number
  agentUiOverrides?: Record<string, Partial<AgentUiConfig>>
  computerTierLlm?: Partial<Record<ComputerTierKey, ComputerTierLlmConfig>>
  /** Debug: per-phase LLM for computer host verify pipeline. */
  computerPipelineLlm?: ComputerPipelineLlmSettings
  /** Debug: per-mode LLM for general / coder agents. */
  agentModeLlm?: AgentModeLlmMap
  /** Debug: per-mode LLM for image / audio / video understanding. */
  mediaModeLlm?: MediaModeLlmMap
  mediaModelOverrides?: MediaModelOverrides
  agentPerformanceModes?: AgentPerformanceModes
  mediaUnderstandingModes?: MediaUnderstandingModes
  /** When false, tool calls in one assistant turn run serially. */
  parallelToolExecutionEnabled?: boolean
  maxParallelToolCalls?: number | null
  maxParallelSubAgents?: number | null
  maxParallelMediaJobs?: number | null
  /** Max concurrent dispatcher runs (chat, webhook, cron, …). Default 4. */
  maxConcurrentRuns?: number
}

export interface EffectiveSettingsView {
  user: UserSettings
  platform: PlatformSettings
  merged: ModelSettings
  canEditPlatform: boolean
  isPlatformAdmin: boolean
}

export interface ModelSettings {
  providers: ProviderConfig[]
  activeProviderId: string
  model: string
  temperature: number
  maxTokens: number
  hasKey: boolean
  toolApprovalMode: 'auto' | 'manual'
  agentMode: AgentMode
  /** Absolute path to project root for coder file tools */
  workspaceRoot: string
  /** When agentMode is single, worker agent id (kebab-case); empty = computer agent */
  leadAgentId: string
  /** Summarize older turns when estimated context exceeds budget */
  contextCompressionEnabled: boolean
  /** Estimated token budget for messages; over this triggers compression when enabled */
  contextBudgetTokens: number
  /** Keep this many most recent user messages (and tail) verbatim */
  contextKeepRecentUserTurns: number
  /** Max tokens for the summarization API call */
  contextSummaryMaxTokens: number
  /** Max tool-call rounds per user message (assistant loop), default 100 */
  maxToolRounds: number
  /** Max tool rounds inside each `run_subagent` / `run_sub_agent` inner loop */
  maxSubAgentToolRounds?: number
  maxSubAgentSpawnDepth?: number
  /** 助手消息上「原始输出」调试入口（代码图标）；含正文通道原始字串与 API reasoning，不在主气泡展示 reasoning */
  rawContentViewEnabled: boolean
  /** Write each LLM request payload to app data `logs/llm_prompts/` (debug) */
  debugDumpLlmPrompts?: boolean
  /** Debug: KEY→VALUE overlays for `terminal` child env (session memory). */
  terminalEnvOverrides?: Record<string, string>
  /** Settings dialog debug sections toggle (independent of rawContentView / dump prompts) */
  debugMenusEnabled?: boolean
  /** Debug: show child task boards under parent board panel. */
  taskBoardShowChildBoards?: boolean
  /** agentId → 该 agent 的默认「服务商 + 模型」（显式存储，不从模型名反推服务商） */
  agentDefaultModels: Record<string, AgentModelRef>
  /** agentId → task_board 更新后是否硬截断较早对话（无 LLM 摘要） */
  agentTaskBoardHistoryTrim?: Record<string, boolean>
  /** Computer agent: default human-like mouse movement (Bézier path + jitter) */
  computerHumanLike?: boolean
  /** Computer agent: starting tier for new conversations */
  computerInitialTier?: ComputerInitialTier
  /** Auto-select primary monitor and follow launch_app window */
  computerAutoSwitchMonitor?: boolean
  /** Show annotated screenshot preview on Computer Use assistant messages */
  computerAnnotatedScreenViewEnabled?: boolean
  captchaSliderOffsetPx?: number
  /** UI color scheme */
  theme?: ThemePreference
  /** Per-agent UI overrides (merged over manifest `ui`) */
  agentUiOverrides?: Record<string, Partial<AgentUiConfig>>
  mediaModelOverrides?: MediaModelOverrides
  agentPerformanceModes?: AgentPerformanceModes
  mediaUnderstandingModes?: MediaUnderstandingModes
  agentModeLlm?: AgentModeLlmMap
  mediaModeLlm?: MediaModeLlmMap
  mediaOss?: MediaOssConfig
  parallelToolExecutionEnabled?: boolean
  maxParallelToolCalls?: number | null
  maxParallelSubAgents?: number | null
  maxParallelMediaJobs?: number | null
  maxConcurrentRuns?: number
}

export interface SkillDef {
  id: string
  name: string
  description: string
  tags: string[]
  /** Catalog responses omit body; instructions load via skill_read. */
  systemPrompt?: string
  toolNames: string[]
  scenario: string
  builtin: boolean
  /** Always empty in catalog responses; resources read on demand via skill_read. */
  resourceFiles?: string[]
  source?: string
  /** `system` = app data bundled; `user` = ~/.pointer/skills */
  provenance?: string
  mutable?: boolean
}

export interface ExternalSkillSource {
  id: string
  label: string
  path: string
  skillCount: number
  skillIds: string[]
}

export interface ExternalSkillsProbeResult {
  shouldPrompt: boolean
  sources: ExternalSkillSource[]
  totalSkills: number
}

export interface SkillImportResult {
  imported: SkillDef[]
  skipped: string[]
}


/** Tool id only; human-readable docs ship as markdown in pointer-core (`tools/*.md`). */
export interface ToolDef {
  name: string
}

/** Annotated desktop JPEG (base64) for UI preview; mirrors pointer-core `ComputerAnnotatedPreview`. */
export interface ComputerAnnotatedPreview {
  imageBase64: string
  /** `image/jpeg` or legacy `image/png`. */
  imageMime?: string
  caption: string
}

/** Chat attachment bytes for bubble preview reload. */
export interface ChatMediaPreview {
  dataBase64: string
  mimeType: string
  fileName: string
}

export interface MonitorWorkArea {
  left: number
  top: number
  width: number
  height: number
}

export interface ComputerMonitor {
  id: string
  left: number
  top: number
  width: number
  height: number
  isPrimary: boolean
  /** Excludes macOS Dock / menu bar when provided by the desktop host. */
  workArea?: MonitorWorkArea
}

export interface ComputerMonitorPickRequest {
  conversationId: string
  messageId: string
  toolCallId: string
  monitors: ComputerMonitor[]
}

export interface TerminalInputRequest {
  requestId: string
  messageId: string
  toolCallId: string
  command?: string
  outputContext?: string
  inputHint?: string
  inputClass: 'normal' | 'secret'
  traceId?: string
  scopedMessageId?: string
}

export interface ContextCompressionInfo {
  reason: 'budget' | 'tool_limit' | string
  messagesBefore: number
  messagesAfter: number
  droppedCount: number
  keepRecentUserTurns: number
  scope: 'main' | 'sub_agent' | string
  subAgentId?: string
  subAgentName?: string
  taskId?: string
}

export type StreamEvent =
  | { kind: 'message_start'; messageId: string; conversationId: string }
  | {
      kind: 'sub_message_start'
      conversationId: string
      anchorMessageId: string
      scopedMessageId: string
      traceId: string
      taskId: string
      spawnDepth: number
      agentInstanceId: string
    }
  | { kind: 'delta'; messageId: string; text: string }
  | { kind: 'raw_content_delta'; messageId: string; text: string; traceId?: string; scopedMessageId?: string }
  | { kind: 'reasoning_delta'; messageId: string; text: string; traceId?: string; scopedMessageId?: string }
  /** 正文里 XML 工具块尚未闭合时，已能读出的子标签（流式更新）。 */
  | {
      kind: 'assistant_json_partial'
      messageId: string
      thoughts?: string
      headline?: string
      toolName?: string
      responseText?: string
      traceId?: string
      scopedMessageId?: string
    }
  | { kind: 'agent_step'; messageId: string; agent: AgentTrace }
  | { kind: 'tool_call_start'; messageId: string; toolCall: ToolCall; traceId?: string; scopedMessageId?: string }
  | { kind: 'tool_call_args_delta'; messageId: string; toolCallId: string; argsDelta: string; traceId?: string; scopedMessageId?: string }
  | {
      kind: 'tool_call_status'
      messageId: string
      toolCallId: string
      status: ToolCall['status']
      result?: string
      error?: string
      durationMs?: number
      displayLabel?: string
      displaySummary?: string
      traceId?: string
      scopedMessageId?: string
    }
  | { kind: 'terminal_output_delta'; messageId: string; toolCallId: string; output: string; traceId?: string; scopedMessageId?: string }
  | { kind: 'console_output_delta'; sessionId: string; workspaceRoot: string; cwd: string; output: string }
  | { kind: 'console_session_exited'; sessionId: string; workspaceRoot: string; cwd: string; exitCode?: number }
  | {
      kind: 'terminal_needs_input'
      messageId: string
      toolCallId: string
      requestId: string
      command?: string
      outputContext?: string
      inputHint?: string
      inputClass: 'normal' | 'secret'
      traceId?: string
      scopedMessageId?: string
    }
  | { kind: 'web_search_output_delta'; messageId: string; toolCallId: string; text: string; traceId?: string; scopedMessageId?: string }
  | {
      kind: 'web_search_sources_ready'
      messageId: string
      toolCallId: string
      sources: WebSearchSourceEntry[]
      searchCount: number
      traceId?: string
      scopedMessageId?: string
    }
  | {
      kind: 'message_end'
      messageId: string
      content?: string
      rawContent?: string
      toolRawOutput?: string
      thoughts?: string
      headline?: string
      traceId?: string
      scopedMessageId?: string
      attachments?: MediaAttachment[]
    }
  | {
      kind: 'injected_user_message'
      conversationId: string
      messageId: string
      content: string
      attachments?: MediaAttachment[]
    }
  | {
      kind: 'im_session_forked'
      conversationId: string
      baseConversationId: string
      title: string
      sessionEpoch: number
      leadAgentId: string
      agentMode: AgentMode
    }
  | {
      kind: 'im_session_agent_changed'
      conversationId: string
      baseConversationId: string
      leadAgentId: string
      agentMode: AgentMode
    }
  | {
      kind: 'user_message_attachments_updated'
      conversationId: string
      messageId: string
      attachments: MediaAttachment[]
      content?: string
    }
  /** App-injected assistant line (e.g. desktop capture status); shown in thread, not from model. */
  | { kind: 'injected_assistant_message'; conversationId: string; messageId: string; content: string }
  /** Same `messageId` as a prior `injected_assistant_message`; updates its `content` only. */
  | { kind: 'injected_assistant_message_update'; conversationId: string; messageId: string; content: string }
  | { kind: 'error'; conversationId?: string; messageId?: string; message: string }
  | { kind: 'done'; conversationId: string; toolRoundsUsedTotal?: number; toolRoundsUsedSupervisorTotal?: number; maxToolRounds?: number }
  | { kind: 'context_trim_applied'; conversationId: string; excludedMessageIds: string[] }
  /** Ephemeral: compression in progress (tool-row marker); not persisted. */
  | {
      kind: 'context_compression_started'
      conversationId: string
      scope: 'main' | 'sub_agent' | string
      messageId?: string
      subAgentId?: string
      subAgentName?: string
    }
  | {
      kind: 'context_compression_applied'
      conversationId: string
      excludedMessageIds: string[]
      insertBeforeMessageId: string
      summaryMessage: ChatMessage
      compression: ContextCompressionInfo
    }
  | { kind: 'context_compressed'; conversationId: string; messageId: string; compression: ContextCompressionInfo }
  | { kind: 'tool_rounds_exhausted'; conversationId: string; maxRounds: number; message: string; willRetryAfterCompress: boolean }
  /** Ephemeral UI only; not saved as a chat message or sent to the model. */
  | { kind: 'ui_toast'; conversationId: string; message: string; level: string }
  /** IM DM pairing code issued — UI may prompt approval (poll only while armed). */
  | {
      kind: 'channel_pairing_pending'
      channel: string
      accountId: string
      code: string
      senderId: string
      issuedAt: number
    }
  /** Annotated screen for one assistant message (path under computer-captures/). */
  | { kind: 'assistant_round_screen'; conversationId: string; messageId: string; annotatedRelPath: string }
  | { kind: 'supervisor_plan'; conversationId: string; messageId: string; tasks: SupervisorPlanTask[] }
  | { kind: 'task_board_updated'; conversationId: string; storeKey: string; anchorMessageId?: string; document: TaskBoardDocument }
  | { kind: 'skills_updated'; conversationId: string; importedIds: string[]; enabledIds?: string[] }
  | { kind: 'workspace_updated'; conversationId: string; workspaceRoot: string; isEphemeralSandbox: boolean }
  | { kind: 'computer_monitor_pick_required'; conversationId: string; messageId: string; toolCallId: string; monitors: ComputerMonitor[] }
  | { kind: 'computer_monitor_updated'; conversationId: string; monitorId?: string | null }

export interface SupervisorPlanTask {
  id: string
  title: string
  agentId: string
}

export type TaskBoardItemStatus =
  | 'pending'
  | 'ready'
  | 'in_progress'
  | 'done'
  | 'cancelled'
  | 'failed'

export interface TaskBoardItem {
  id: string
  title: string
  status: TaskBoardItemStatus
  depends_on?: string[]
  retry_count?: number
  plan?: string
  done_when?: string
  remark?: string
  constraint?: string
  /** Milestone position, e.g. `3/10` or `batch 2/4`. Legacy boards may still send `checkpoint`. */
  progress?: string
  checkpoint?: string
  validate_requirement?: string
  validate_results?: string[]
  extract_requirement?: string
  extract_results?: string[]
  blocked_by?: string
  work_item_mode?: 'enumerated' | 'dynamic'
  dynamic_quota?: number
  delivery_format?: 'xlsx' | 'csv' | 'txt' | 'jsonl'
}

export interface TaskBoardGlobalContext {
  key_findings?: string[]
  artifacts?: Record<string, unknown>
}

export interface TaskBoardDocument {
  version: number
  task_id: string
  meta: {
    goal: string
    status: string
    max_depth?: number
    expected_total?: number
    done_when?: string
    constraint?: string
    context?: string
    work_item_mode?: 'enumerated' | 'dynamic'
    dynamic_quota?: number
    work_items_source_path?: string
    work_items_seeded_rows?: number
    scope?: 'parent' | 'child'
    root_target?: string
    parent_sub_task_id?: string
    parent_store_key?: string
  }
  global_context?: TaskBoardGlobalContext
  /** v4 global milestone rows (`g_plan` / `g_exec` / Type1 steps). */
  global_milestones?: TaskBoardItem[]
  /** v4 reusable per-item SOP template. */
  item_milestones?: TaskBoardItem[]
  /** Legacy v3 alias of `global_milestones`. */
  board?: TaskBoardItem[]
}
