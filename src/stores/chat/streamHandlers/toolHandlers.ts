import {
  clearToolArgsDeltaBufferForTool,
  enqueueToolArgsDelta,
  enqueueToolOutputDelta,
  enqueueWebSearchOutputDelta
} from '../../../lib/reasoningDeltaBatch'
import { resolveStreamWriteMessage } from '../../../lib/subAgentMessages'
import { ensureSubTrace, ensureSubTraceSession, recordSubToolSuccess } from '../../../lib/subAgentSession'
import { formatTerminalOutputContext } from '../../../lib/terminalOutputContext'
import { isBackgroundJobHost, isToolCallInProgress } from '../../../lib/toolCallDisplay'
import type { StreamEvent, TaskBoardDocument, ToolCall } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type ToolCallStart = Extract<StreamEvent, { kind: 'tool_call_start' }>

function upsertToolCall(toolCalls: ToolCall[] | undefined, incoming: ToolCall): ToolCall[] {
  const list = toolCalls || []
  const existing = list.find(t => t.id === incoming.id)
  if (!existing) {
    list.push({ ...incoming })
  } else {
    // Finish may re-emit ToolCallStart to refresh arguments; don't clobber outcomes.
    const prevStatus = existing.status
    const prevResult = existing.result
    const prevError = existing.error
    const prevDuration = existing.durationMs
    Object.assign(existing, incoming)
    if (prevStatus === 'success' || prevStatus === 'failed' || prevStatus === 'rejected') {
      existing.status = prevStatus
      existing.result = prevResult
      existing.error = prevError
      existing.durationMs = prevDuration
    }
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

function resolveToolCallForStream(
  ctx: StreamHandlerContext,
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
): ToolCall | undefined {
  const r = ctx.findMessage(messageId)
  if (!r) return undefined
  const target = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
  if (target) {
    return findToolCallOnMessage(target, toolCallId)
  }
  if (traceId?.trim()) {
    return ensureSubTrace(r.msg, traceId.trim()).session?.toolCalls?.find(t => t.id === toolCallId)
  }
  return findToolCallOnMessage(r.msg, toolCallId)
}

function terminalInputRequestMatches(
  req: { messageId: string; toolCallId: string; traceId?: string; scopedMessageId?: string },
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
): boolean {
  if (req.messageId !== messageId || req.toolCallId !== toolCallId) return false
  if (req.traceId?.trim() && traceId?.trim() && req.traceId.trim() !== traceId.trim()) return false
  if (
    req.scopedMessageId?.trim()
    && scopedMessageId?.trim()
    && req.scopedMessageId.trim() !== scopedMessageId.trim()
  ) {
    return false
  }
  return true
}

function syncTerminalInputOutputContext(
  ctx: StreamHandlerContext,
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
) {
  const req = ctx.terminalInputRequest.value
  if (!req || !terminalInputRequestMatches(req, messageId, toolCallId, traceId, scopedMessageId)) {
    return
  }
  const tc = resolveToolCallForStream(ctx, messageId, toolCallId, traceId, scopedMessageId)
  const outputContext = formatTerminalOutputContext(tc?.terminalOutput ?? '')
  if (outputContext === req.outputContext) return
  ctx.terminalInputRequest.value = { ...req, outputContext }
}

export function handleToolCallStart(ctx: StreamHandlerContext, e: ToolCallStart) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  // Authoritative snapshot already includes prior deltas; discard batched leftovers.
  if (e.toolCall.arguments?.trim()) {
    clearToolArgsDeltaBufferForTool(e.messageId, e.toolCall.id, e.traceId, e.scopedMessageId)
  }
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    target.toolCalls = upsertToolCall(target.toolCalls, e.toolCall)
    if (target.status !== 'cancelled' && target.status !== 'error') {
      target.contentStreaming = true
      target.status = 'streaming'
    }
    ctx.notifyScopedStreamWrite(r.conv, r.msg, target, e.traceId)
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = ensureSubTraceSession(trace)
    session.toolCalls = upsertToolCall(session.toolCalls, e.toolCall)
    session.contentStreaming = true
    ctx.notifyScopedStreamWrite(r.conv, r.msg, null, e.traceId)
    return
  }
  if (r.msg.status !== 'cancelled' && r.msg.status !== 'error') {
    r.msg.status = 'streaming'
  }
  r.msg.toolCalls = upsertToolCall(r.msg.toolCalls, e.toolCall)
}

export function handleToolCallArgsDelta(ctx: StreamHandlerContext, e: ToolCallArgsDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  enqueueToolArgsDelta(
    e.messageId,
    e.toolCallId,
    e.argsDelta,
    e.traceId,
    e.scopedMessageId
  )
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

function maybeClearBackgroundOccupancy(
  ctx: StreamHandlerContext,
  conversationId: string,
  tc: ToolCall
) {
  if (!isBackgroundJobHost(tc)) return
  if (isToolCallInProgress(tc.status)) return
  ctx.clearBackgroundJobsIfNoneLive(conversationId)
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
      maybeClearBackgroundOccupancy(ctx, r.conv.id, tc)
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
      maybeClearBackgroundOccupancy(ctx, r.conv.id, tc)
    }
  }
  ctx.handleTerminalToolCallStatus(
    e.messageId,
    e.toolCallId,
    e.status,
    e.traceId,
    e.scopedMessageId
  )
  ctx.notifyScopedStreamWrite(r.conv, r.msg, target, e.traceId)
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
    command: e.command?.trim() || undefined,
    outputContext: e.outputContext?.trim() || undefined,
    inputHint: e.inputHint,
    inputClass: e.inputClass,
    traceId: e.traceId,
    scopedMessageId: e.scopedMessageId
  }
  syncTerminalInputOutputContext(ctx, e.messageId, e.toolCallId, e.traceId, e.scopedMessageId)
  ctx.showUiToast(
    e.inputClass === 'secret' ? '终端命令需要密码，请在弹窗中输入' : '终端命令等待你的输入',
    'warning'
  )
}

export function handleTerminalOutputDelta(ctx: StreamHandlerContext, e: TerminalOutputDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  enqueueToolOutputDelta(
    e.messageId,
    e.toolCallId,
    e.output,
    e.traceId,
    e.scopedMessageId
  )
  ctx.syncTerminalLivePopupOutput(e.messageId, e.toolCallId, e.traceId, e.scopedMessageId)
  syncTerminalInputOutputContext(ctx, e.messageId, e.toolCallId, e.traceId, e.scopedMessageId)
}

export function handleWebSearchOutputDelta(ctx: StreamHandlerContext, e: WebSearchOutputDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  enqueueWebSearchOutputDelta(
    e.messageId,
    e.toolCallId,
    e.text,
    e.traceId,
    e.scopedMessageId
  )
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
