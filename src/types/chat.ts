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
  toolCallId?: string // when role = 'tool'
  errorMessage?: string
  reasoning?: string
  agentId?: string
  agentName?: string
  agentTrace?: AgentTrace[]
}

export interface Conversation {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  messages: ChatMessage[]
  skillIds: string[]
}

export interface ProviderConfig {
  id: string
  name: string
  baseUrl: string
  apiKey: string
  models: string[]
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


export interface ToolDef {
  name: string
  description: string
  parametersSchema: Record<string, unknown>
  riskLevel: 'low' | 'medium' | 'high'
  requiresApproval: boolean
}

export type StreamEvent =
  | { kind: 'message_start'; messageId: string; conversationId: string }
  | { kind: 'delta'; messageId: string; text: string }
  | { kind: 'reasoning_delta'; messageId: string; text: string }
  | { kind: 'agent_step'; messageId: string; agent: AgentTrace }
  | { kind: 'tool_call_start'; messageId: string; toolCall: ToolCall }
  | { kind: 'tool_call_args_delta'; messageId: string; toolCallId: string; argsDelta: string }
  | { kind: 'tool_call_status'; messageId: string; toolCallId: string; status: ToolCall['status']; result?: string; error?: string; durationMs?: number }
  | { kind: 'terminal_output_delta'; messageId: string; toolCallId: string; output: string }
  | { kind: 'message_end'; messageId: string }
  | { kind: 'error'; messageId?: string; message: string }
  | { kind: 'done'; conversationId: string }
