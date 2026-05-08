export type Role = 'system' | 'user' | 'assistant' | 'tool'

export type MessageStatus = 'pending' | 'streaming' | 'done' | 'error' | 'cancelled'

export interface ToolCall {
  id: string
  name: string
  arguments: string // JSON string (may be partial during streaming)
  status: 'pending' | 'pending_approval' | 'running' | 'success' | 'failed' | 'rejected'
  result?: string
  error?: string
  durationMs?: number
  riskLevel?: 'low' | 'medium' | 'high'
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
}

export interface Conversation {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  messages: ChatMessage[]
  skillIds: string[]
}

export interface ModelSettings {
  provider: string
  baseUrl: string
  model: string
  apiKey: string // empty in frontend; backend stores actual key
  temperature: number
  maxTokens: number
  hasKey: boolean
  toolApprovalMode: 'auto' | 'manual'
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
  | { kind: 'tool_call_start'; messageId: string; toolCall: ToolCall }
  | { kind: 'tool_call_args_delta'; messageId: string; toolCallId: string; argsDelta: string }
  | { kind: 'tool_call_status'; messageId: string; toolCallId: string; status: ToolCall['status']; result?: string; error?: string; durationMs?: number }
  | { kind: 'message_end'; messageId: string }
  | { kind: 'error'; messageId?: string; message: string }
  | { kind: 'done'; conversationId: string }
