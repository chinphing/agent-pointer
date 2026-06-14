import { ensureSubTrace, recordSubToolSuccess } from '../../../lib/subAgentSession'
import type { StreamEvent, TaskBoardDocument, ToolCall } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type ToolCallStart = Extract<StreamEvent, { kind: 'tool_call_start' }>

function upsertToolCall(toolCalls: ToolCall[] | undefined, incoming: ToolCall): ToolCall[] {
  const list = toolCalls || []
  const existing = list.find(t => t.id === incoming.id)
  if (!existing) {
    list.push({ ...incoming })
  } else {
    Object.assign(existing, incoming)
  }
  return list
}

type ToolCallArgsDelta = Extract<StreamEvent, { kind: 'tool_call_args_delta' }>
type ToolCallStatus = Extract<StreamEvent, { kind: 'tool_call_status' }>
type TerminalOutputDelta = Extract<StreamEvent, { kind: 'terminal_output_delta' }>
type WebSearchOutputDelta = Extract<StreamEvent, { kind: 'web_search_output_delta' }>
type WebSearchSourcesReady = Extract<StreamEvent, { kind: 'web_search_sources_ready' }>

export function handleToolCallStart(ctx: StreamHandlerContext, e: ToolCallStart) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = trace.session!
    session.toolCalls = upsertToolCall(session.toolCalls, e.toolCall)
    session.contentStreaming = true
  } else {
    r.msg.status = 'streaming'
    r.msg.toolCalls = upsertToolCall(r.msg.toolCalls, e.toolCall)
  }
}

export function handleToolCallArgsDelta(ctx: StreamHandlerContext, e: ToolCallArgsDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.arguments += e.argsDelta
  } else {
    const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.arguments += e.argsDelta
  }
}

export function handleToolCallStatus(ctx: StreamHandlerContext, e: ToolCallStatus) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = trace.session!
    const tc = session.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) {
      tc.status = e.status
      if (e.result !== undefined) tc.result = e.result
      if (e.error !== undefined) tc.error = e.error
      if (e.durationMs !== undefined) tc.durationMs = e.durationMs
      if (e.displayLabel !== undefined) tc.displayLabel = e.displayLabel
      if (e.displaySummary !== undefined) tc.displaySummary = e.displaySummary
      if (e.status === 'success') recordSubToolSuccess(session, tc.name, tc.arguments)
    }
  } else {
    const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) {
      tc.status = e.status
      if (e.result !== undefined) tc.result = e.result
      if (e.error !== undefined) tc.error = e.error
      if (e.durationMs !== undefined) tc.durationMs = e.durationMs
      if (e.displayLabel !== undefined) tc.displayLabel = e.displayLabel
      if (e.displaySummary !== undefined) tc.displaySummary = e.displaySummary
    }
  }
  ctx.handleTerminalToolCallStatus(e.messageId, e.toolCallId, e.status, e.traceId)
}

export function handleTerminalOutputDelta(ctx: StreamHandlerContext, e: TerminalOutputDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.terminalOutput = (tc.terminalOutput || '') + e.output
  } else {
    const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.terminalOutput = (tc.terminalOutput || '') + e.output
  }
  ctx.syncTerminalLivePopupOutput(e.messageId, e.toolCallId, e.traceId)
}

export function handleWebSearchOutputDelta(ctx: StreamHandlerContext, e: WebSearchOutputDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + e.text
  } else {
    const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + e.text
  }
}

export function handleWebSearchSourcesReady(ctx: StreamHandlerContext, e: WebSearchSourcesReady) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.webSearchSources = e.sources
  } else {
    const tc = r.msg.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.webSearchSources = e.sources
  }
}

export function handleTaskBoardUpdated(ctx: StreamHandlerContext, e: Extract<StreamEvent, { kind: 'task_board_updated' }>) {
  if (!e.conversationId) return
  ctx.applyTaskBoardDocumentDebounced(
    e.conversationId,
    e.storeKey,
    e.document as TaskBoardDocument,
    e.anchorMessageId
  )
}
