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
}

export const DEFAULT_LEAD_AGENT_ID = 'computer'

export type AgentMode = 'single' | 'supervisor'

export type ThemePreference = 'light' | 'dark' | 'system'

/** Computer agent vision tier (matches backend `ComputerTier`). */
export type ComputerInitialTier = 'primary' | 'intermediate' | 'advanced'

export const COMPUTER_INITIAL_TIER_OPTIONS: { value: ComputerInitialTier; label: string }[] = [
  { value: 'primary', label: '初级' },
  { value: 'intermediate', label: '中级' },
  { value: 'advanced', label: '高级' }
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
  searchCount: number
  readCount: number
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

export interface AgentTrace {
  id: string
  name: string
  role: string
  status: string
  detail?: string
  content?: string
  /** 0 = 顶格（主编排），1 = 委托子 Agent；缺省时 UI 对首条顶格、其余一级缩进 */
  depth?: number
  session?: SubAgentSessionUi
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
  allowSkills: string[]
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

export type ExcludedReason = 'context_compression' | 'task_board_trim'

export interface MessageContextState {
  included: boolean
  excludedReason?: ExcludedReason
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
}

export interface Conversation {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  messages: ChatMessage[]
  skillIds: string[]
  /** Cumulative tool rounds for single-agent replies (cap in settings). */
  toolRoundsUsed?: number
  /** Cumulative tool rounds for Supervisor / sub-agents (separate cap pool). */
  toolRoundsUsedSupervisor?: number
  /** Selected desktop monitor for Computer agent; empty = auto (monitor under cursor). */
  computerMonitorId?: string
  /** Per-conversation workspace for coder/file tools (set in composer). */
  workspaceRoot?: string
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
}

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
}

export interface ComputerTierLlmConfig {
  providerId: string
  model: string
  enableThinking?: boolean
  thinkingBudget?: number
}

export type ComputerTierKey = 'primary' | 'intermediate' | 'advanced'

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
  rawContentViewEnabled: boolean
  /** Write each LLM request payload to app data `logs/llm_prompts/` (debug) */
  debugDumpLlmPrompts?: boolean
  /** Settings dialog debug sections toggle (independent of rawContentView / dump prompts) */
  debugMenusEnabled?: boolean
  /** Debug: show child task boards under parent board panel. */
  taskBoardShowChildBoards?: boolean
  agentDefaultModels: Record<string, AgentModelRef>
  agentTaskBoardHistoryTrim?: Record<string, boolean>
  computerHumanLike?: boolean
  computerInitialTier?: ComputerInitialTier
  /** Show annotated screenshot preview on Computer Use assistant messages */
  computerAnnotatedScreenViewEnabled?: boolean
  /** Pixel offset added to final slider CAPTCHA drag point */
  captchaSliderOffsetPx?: number
  agentUiOverrides?: Record<string, Partial<AgentUiConfig>>
  computerTierLlm?: Partial<Record<ComputerTierKey, ComputerTierLlmConfig>>
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
  /** 助手消息上「原始输出」调试入口（代码图标）；含正文通道原始字串与 API reasoning，不在主气泡展示 reasoning */
  rawContentViewEnabled: boolean
  /** Write each LLM request payload to app data `logs/llm_prompts/` (debug) */
  debugDumpLlmPrompts?: boolean
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
  /** Show annotated screenshot preview on Computer Use assistant messages */
  computerAnnotatedScreenViewEnabled?: boolean
  captchaSliderOffsetPx?: number
  /** UI color scheme */
  theme?: ThemePreference
  /** Per-agent UI overrides (merged over manifest `ui`) */
  agentUiOverrides?: Record<string, Partial<AgentUiConfig>>
}

export interface SkillDef {
  id: string
  name: string
  description: string
  tags: string[]
  systemPrompt: string
  toolNames: string[]
  scenario: string
  builtin: boolean
  resourceFiles: string[]
  source?: string
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

export interface ComputerMonitor {
  id: string
  left: number
  top: number
  width: number
  height: number
  isPrimary: boolean
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
  | { kind: 'delta'; messageId: string; text: string }
  | { kind: 'raw_content_delta'; messageId: string; text: string; traceId?: string }
  | { kind: 'reasoning_delta'; messageId: string; text: string; traceId?: string }
  /** 正文里 XML 工具块尚未闭合时，已能读出的子标签（流式更新）。 */
  | { kind: 'assistant_json_partial'; messageId: string; thoughts?: string; headline?: string; toolName?: string; responseText?: string; traceId?: string }
  | { kind: 'agent_step'; messageId: string; agent: AgentTrace }
  | { kind: 'tool_call_start'; messageId: string; toolCall: ToolCall; traceId?: string }
  | { kind: 'tool_call_args_delta'; messageId: string; toolCallId: string; argsDelta: string; traceId?: string }
  | { kind: 'tool_call_status'; messageId: string; toolCallId: string; status: ToolCall['status']; result?: string; error?: string; durationMs?: number; displayLabel?: string; displaySummary?: string; traceId?: string }
  | { kind: 'terminal_output_delta'; messageId: string; toolCallId: string; output: string; traceId?: string }
  | { kind: 'web_search_output_delta'; messageId: string; toolCallId: string; text: string; traceId?: string }
  | { kind: 'web_search_sources_ready'; messageId: string; toolCallId: string; sources: WebSearchSourceEntry[]; searchCount: number; traceId?: string }
  | { kind: 'message_end'; messageId: string; content?: string; rawContent?: string; toolRawOutput?: string; thoughts?: string; headline?: string; traceId?: string }
  | { kind: 'injected_user_message'; conversationId: string; messageId: string; content: string }
  /** App-injected assistant line (e.g. desktop capture status); shown in thread, not from model. */
  | { kind: 'injected_assistant_message'; conversationId: string; messageId: string; content: string }
  /** Same `messageId` as a prior `injected_assistant_message`; updates its `content` only. */
  | { kind: 'injected_assistant_message_update'; conversationId: string; messageId: string; content: string }
  | { kind: 'error'; messageId?: string; message: string }
  | { kind: 'done'; conversationId: string; toolRoundsUsedTotal?: number; toolRoundsUsedSupervisorTotal?: number; maxToolRounds?: number }
  | { kind: 'history_replaced'; conversationId: string; messages: ChatMessage[]; compression?: ContextCompressionInfo }
  | { kind: 'context_compressed'; conversationId: string; messageId: string; compression: ContextCompressionInfo }
  | { kind: 'tool_rounds_exhausted'; conversationId: string; maxRounds: number; message: string; willRetryAfterCompress: boolean }
  /** Ephemeral UI only; not saved as a chat message or sent to the model. */
  | { kind: 'ui_toast'; conversationId: string; message: string; level: string }
  /** Annotated screen for one assistant message (path under computer-captures/). */
  | { kind: 'assistant_round_screen'; conversationId: string; messageId: string; annotatedRelPath: string }
  | { kind: 'supervisor_plan'; conversationId: string; messageId: string; tasks: SupervisorPlanTask[] }
  | { kind: 'task_board_updated'; conversationId: string; storeKey: string; anchorMessageId?: string; document: TaskBoardDocument }

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
  /** Milestone position, e.g. `3/10` or `batch 2/4`. Legacy boards may still send `checkpoint`. */
  progress?: string
  checkpoint?: string
  validate_requirement?: string
  validate_results?: string[]
  extract_requirement?: string
  extract_results?: string[]
  blocked_by?: string
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
    step_count?: number
    max_steps?: number
    max_depth?: number
    expected_total?: number
    scope?: 'parent' | 'child'
    root_target?: string
    parent_sub_task_id?: string
    parent_store_key?: string
  }
  global_context?: TaskBoardGlobalContext
  board: TaskBoardItem[]
}
