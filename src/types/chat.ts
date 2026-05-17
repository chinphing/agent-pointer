export type Role = 'system' | 'user' | 'assistant' | 'tool'

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
}

export type AgentMode = 'single' | 'supervisor'

export interface AgentTrace {
  id: string
  name: string
  role: string
  status: string
  detail?: string
  content?: string
  /** 0 = 顶格（主编排），1 = 委托子 Agent；缺省时 UI 对首条顶格、其余一级缩进 */
  depth?: number
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
  rawContent?: string
  agentId?: string
  agentName?: string
  agentTrace?: AgentTrace[]
  /** Annotated PNG path under app `computer-captures/` (lazy load on preview); persisted when the stream emits it. */
  computerRoundScreenRelPath?: string
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
}

/** Per-model API/runtime overrides; unset fields inherit from the parent provider. */
export interface ModelRuntimeOverrides {
  reasoningInMessages?: boolean
  temperature?: number
  maxTokens?: number
  /** Merged into chat request top-level `extra_body` (JSON object); shallow-merge over provider default. */
  extraBody?: Record<string, unknown>
}

export interface ProviderConfig {
  id: string
  name: string
  baseUrl: string
  apiKey: string
  models: string[]
  /** Default for all models under this provider when `modelConfigs[model]` has no override. */
  reasoningInMessages?: boolean
  modelConfigs?: Record<string, ModelRuntimeOverrides>
  /** Default `extra_body` for chat/completions (JSON object). */
  extraBody?: Record<string, unknown>
}

export interface AgentModelRef {
  providerId: string
  model: string
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
  /** When agentMode is single, worker agent id (kebab-case); empty = default agent */
  leadAgentId: string
  /** Summarize older turns when estimated context exceeds budget */
  contextCompressionEnabled: boolean
  /** Rough character budget for messages; over this triggers compression when enabled */
  contextBudgetChars: number
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
  /** agentId → 该 agent 的默认「服务商 + 模型」（显式存储，不从模型名反推服务商） */
  agentDefaultModels: Record<string, AgentModelRef>
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

/** Annotated desktop PNG (base64) for UI preview; mirrors pointer-core `ComputerAnnotatedPreview`. */
export interface ComputerAnnotatedPreview {
  imageBase64: string
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
  | { kind: 'raw_content_delta'; messageId: string; text: string }
  | { kind: 'reasoning_delta'; messageId: string; text: string }
  /** 正文里 XML 工具块尚未闭合时，已能读出的子标签（流式更新）。 */
  | { kind: 'assistant_json_partial'; messageId: string; thoughts?: string; headline?: string; toolName?: string; responseText?: string }
  | { kind: 'agent_step'; messageId: string; agent: AgentTrace }
  | { kind: 'tool_call_start'; messageId: string; toolCall: ToolCall }
  | { kind: 'tool_call_args_delta'; messageId: string; toolCallId: string; argsDelta: string }
  | { kind: 'tool_call_status'; messageId: string; toolCallId: string; status: ToolCall['status']; result?: string; error?: string; durationMs?: number }
  | { kind: 'terminal_output_delta'; messageId: string; toolCallId: string; output: string }
  | { kind: 'message_end'; messageId: string; content?: string; rawContent?: string; thoughts?: string; headline?: string }
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
