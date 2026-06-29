import { resolveStreamWriteMessage } from '../../../lib/subAgentMessages'
import { ensureSubTrace, ensureSubTraceSession, recordSubToolSuccess } from '../../../lib/subAgentSession'
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
type TerminalNeedsInput = Extract<StreamEvent, { kind: 'terminal_needs_input' }>
type WebSearchOutputDelta = Extract<StreamEvent, { kind: 'web_search_output_delta' }>
type WebSearchSourcesReady = Extract<StreamEvent, { kind: 'web_search_sources_ready' }>

function findToolCallOnMessage(msg: { toolCalls?: ToolCall[] }, toolCallId: string): ToolCall | undefined {
  return msg.toolCalls?.find(t => t.id === toolCallId)
}

export function handleToolCallStart(ctx: StreamHandlerContext, e: ToolCallStart) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    target.toolCalls = upsertToolCall(target.toolCalls, e.toolCall)
    target.contentStreaming = true
    target.status = 'streaming'
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = ensureSubTraceSession(trace)
    session.toolCalls = upsertToolCall(session.toolCalls, e.toolCall)
    session.contentStreaming = true
    return
  }
  r.msg.status = 'streaming'
  r.msg.toolCalls = upsertToolCall(r.msg.toolCalls, e.toolCall)
}

export function handleToolCallArgsDelta(ctx: StreamHandlerContext, e: ToolCallArgsDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    const tc = findToolCallOnMessage(target, e.toolCallId)
    if (tc) tc.arguments += e.argsDelta
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.arguments += e.argsDelta
    return
  }
  const tc = findToolCallOnMessage(r.msg, e.toolCallId)
  if (tc) tc.arguments += e.argsDelta
}

function markToolCallWaitingForInput(
  ctx: StreamHandlerContext,
  messageId: string,
  toolCallId: string,
  waiting: boolean,
  traceId?: string,
  scopedMessageId?: string
) {
  const r = ctx.findMessage(messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
  let tc: ToolCall | undefined
  if (target) {
    tc = findToolCallOnMessage(target, toolCallId)
  } else if (traceId?.trim()) {
    tc = ensureSubTrace(r.msg, traceId.trim()).session?.toolCalls?.find(t => t.id === toolCallId)
  } else {
    tc = findToolCallOnMessage(r.msg, toolCallId)
  }
  if (tc) tc.waitingForInput = waiting
}

export function handleToolCallStatus(ctx: StreamHandlerContext, e: ToolCallStatus) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    const tc = findToolCallOnMessage(target, e.toolCallId)
    if (tc) {
      tc.status = e.status
      if (e.result !== undefined) tc.result = e.result
      if (e.error !== undefined) tc.error = e.error
      if (e.durationMs !== undefined) tc.durationMs = e.durationMs
      if (e.displayLabel !== undefined) tc.displayLabel = e.displayLabel
      if (e.displaySummary !== undefined) tc.displaySummary = e.displaySummary
    }
  } else if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = ensureSubTraceSession(trace)
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
    const tc = findToolCallOnMessage(r.msg, e.toolCallId)
    if (tc) {
      tc.status = e.status
      if (e.result !== undefined) tc.result = e.result
      if (e.error !== undefined) tc.error = e.error
      if (e.durationMs !== undefined) tc.durationMs = e.durationMs
      if (e.displayLabel !== undefined) tc.displayLabel = e.displayLabel
      if (e.displaySummary !== undefined) tc.displaySummary = e.displaySummary
    }
  }
  ctx.handleTerminalToolCallStatus(
    e.messageId,
    e.toolCallId,
    e.status,
    e.traceId,
    e.scopedMessageId
  )
  if (e.status !== 'running') {
    markToolCallWaitingForInput(
      ctx,
      e.messageId,
      e.toolCallId,
      false,
      e.traceId,
      e.scopedMessageId
    )
    ctx.clearTerminalInputRequest(e.toolCallId)
  }
}

export function handleTerminalNeedsInput(ctx: StreamHandlerContext, e: TerminalNeedsInput) {
  ctx.dismissTerminalLivePopup()
  markToolCallWaitingForInput(
    ctx,
    e.messageId,
    e.toolCallId,
    true,
    e.traceId,
    e.scopedMessageId
  )
  ctx.terminalInputRequest.value = {
    requestId: e.requestId,
    messageId: e.messageId,
    toolCallId: e.toolCallId,
    inputHint: e.inputHint,
    inputClass: e.inputClass,
    traceId: e.traceId,
    scopedMessageId: e.scopedMessageId
  }
  ctx.showUiToast(
    e.inputClass === 'secret' ? '终端命令需要密码，请在弹窗中输入' : '终端命令等待你的输入',
    'warning'
  )
}

export function handleTerminalOutputDelta(ctx: StreamHandlerContext, e: TerminalOutputDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    const tc = findToolCallOnMessage(target, e.toolCallId)
    if (tc) tc.terminalOutput = (tc.terminalOutput || '') + e.output
  } else if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.terminalOutput = (tc.terminalOutput || '') + e.output
  } else {
    const tc = findToolCallOnMessage(r.msg, e.toolCallId)
    if (tc) tc.terminalOutput = (tc.terminalOutput || '') + e.output
  }
  ctx.syncTerminalLivePopupOutput(e.messageId, e.toolCallId, e.traceId, e.scopedMessageId)
}

export function handleWebSearchOutputDelta(ctx: StreamHandlerContext, e: WebSearchOutputDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    const tc = findToolCallOnMessage(target, e.toolCallId)
    if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + e.text
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + e.text
    return
  }
  const tc = findToolCallOnMessage(r.msg, e.toolCallId)
  if (tc) tc.webSearchOutput = (tc.webSearchOutput || '') + e.text
}

export function handleWebSearchSourcesReady(ctx: StreamHandlerContext, e: WebSearchSourcesReady) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    const tc = findToolCallOnMessage(target, e.toolCallId)
    if (tc) tc.webSearchSources = e.sources
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const tc = trace.session?.toolCalls?.find(t => t.id === e.toolCallId)
    if (tc) tc.webSearchSources = e.sources
    return
  }
  const tc = findToolCallOnMessage(r.msg, e.toolCallId)
  if (tc) tc.webSearchSources = e.sources
}

export function handleTaskBoardUpdated(ctx: StreamHandlerContext, e: Extract<StreamEvent, { kind: 'task_board_updated' }>) {
  if (!e.conversationId) return
  ctx.applyTaskBoardDocument(
    e.conversationId,
    e.storeKey,
    e.document as TaskBoardDocument,
    e.anchorMessageId
  )
}
