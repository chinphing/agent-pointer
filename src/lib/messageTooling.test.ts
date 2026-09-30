import { describe, expect, it } from 'vitest'
import {
  agentTraceNeedsCollapsedSurface,
  hostNeedsCollapsedSubAgentFrames,
  isCollapsedSurfaceToolCall,
  isInFlightSubagentHostToolCall,
  isPendingApprovalToolCall,
  isPendingAskUserToolCall,
  isSidecarToolCall,
  taskBoardPatchSummaryFromArgs,
  taskBoardToolSummary,
  toolCallBaseName,
  visibleToolCalls
} from './messageTooling'
import type { ToolCall } from '../types/chat'

describe('messageTooling', () => {
  it('derives base name before a colon', () => {
    expect(toolCallBaseName('file_read')).toBe('file_read')
    expect(toolCallBaseName('file_read:x')).toBe('file_read')
    expect(toolCallBaseName('verify:step-1')).toBe('verify')
    expect(toolCallBaseName('task_board_patch')).toBe('task_board_patch')
  })

  it('keeps approval and in-flight hosts on the collapsed surface, never pending ask_user', () => {
    const host: ToolCall = {
      id: '1',
      name: 'run_subagent',
      status: 'running',
      arguments: '{}'
    }
    expect(isInFlightSubagentHostToolCall(host)).toBe(true)
    expect(isCollapsedSurfaceToolCall(host)).toBe(true)
    expect(isPendingApprovalToolCall(host)).toBe(false)
    expect(isCollapsedSurfaceToolCall({
      id: '2',
      name: 'run_subagent',
      status: 'success',
      arguments: '{}'
    })).toBe(false)
    // A running child keeps its frame; a pending ask_user alone does not.
    expect(isCollapsedSurfaceToolCall({
      id: '2',
      name: 'run_subagent',
      status: 'success',
      arguments: '{}'
    }, {
      agentTrace: [{
        status: 'running',
        parentToolCallId: '2',
        session: {
          toolCalls: [{
            id: 'ask',
            name: 'ask_user',
            status: 'pending',
            arguments: '{}'
          }]
        }
      }]
    })).toBe(true)
    expect(isCollapsedSurfaceToolCall({
      id: '3',
      name: 'run_subagent',
      status: 'success',
      arguments: '{}'
    }, {
      agentTrace: [{
        status: 'done',
        parentToolCallId: '3',
        session: {
          toolCalls: [{
            id: 'ask',
            name: 'ask_user',
            status: 'pending',
            arguments: '{}'
          }]
        }
      }]
    })).toBe(false)
    // The banner owns pending ask_user: it never holds a collapsed row itself.
    expect(isPendingAskUserToolCall({
      id: 'ask',
      name: 'ask_user',
      status: 'pending',
      arguments: '{}'
    })).toBe(true)
    expect(isCollapsedSurfaceToolCall({
      id: 'ask',
      name: 'ask_user',
      status: 'pending',
      arguments: '{}'
    })).toBe(false)
    expect(isPendingApprovalToolCall({
      id: 'appr',
      name: 'terminal',
      status: 'pending_approval',
      arguments: '{}'
    })).toBe(true)
    expect(isCollapsedSurfaceToolCall({
      id: 'appr',
      name: 'terminal',
      status: 'pending_approval',
      arguments: '{}'
    })).toBe(true)
    expect(agentTraceNeedsCollapsedSurface([{
      status: 'running',
      session: {
        toolCalls: [{
          id: 'ask',
          name: 'ask_user',
          status: 'pending',
          arguments: '{}'
        }]
      }
    }])).toBe(true)
    expect(agentTraceNeedsCollapsedSurface([{
      status: 'done',
      session: {
        toolCalls: [{
          id: 'ask',
          name: 'ask_user',
          status: 'pending',
          arguments: '{}'
        }]
      }
    }])).toBe(false)
  })

  it('keeps a finished run_subagent frame when the child lives on a trailing group', () => {
    const host: ToolCall = {
      id: 'host',
      name: 'run_subagent',
      status: 'success',
      arguments: '{}'
    }
    const ask: ToolCall = {
      id: 'ask',
      name: 'ask_user',
      status: 'running',
      arguments: '{}'
    }
    const child = {
      agentTrace: [{
        status: 'running',
        parentToolCallId: 'host',
        session: { toolCalls: [ask] }
      }]
    }
    expect(hostNeedsCollapsedSubAgentFrames({ toolCalls: [] }, [{
      toolCalls: [host],
      message: child
    }])).toBe(true)
    expect(hostNeedsCollapsedSubAgentFrames({
      toolCalls: [{
        id: 'done',
        name: 'file_read',
        status: 'success',
        arguments: '{}'
      }]
    })).toBe(false)
  })

  it('classifies task_board and verify tools as sidecar', () => {
    expect(isSidecarToolCall('task_board_patch')).toBe(true)
    expect(isSidecarToolCall('task_board_init')).toBe(true)
    expect(isSidecarToolCall('verify:step-1')).toBe(true)
    expect(isSidecarToolCall('ask_user')).toBe(false)
    expect(isSidecarToolCall('file_read')).toBe(false)
  })

  it('does not treat ask_user result as a task board summary', () => {
    // Regression: ask_user returns {"selected": ...}; it must NOT become "任务板 · update".
    expect(taskBoardToolSummary('{"selected":["选项A","选项B"]}')).toBeNull()
    expect(taskBoardToolSummary('{"selected":null,"timed_out":true,"note":"超时"}')).toBeNull()
  })

  it('summarizes task board patch results', () => {
    expect(
      taskBoardToolSummary('{"patched":[{"id":"m1","status":"done"}]}')
    ).toBe('#m1 → done')
    expect(
      taskBoardToolSummary('{"patched":[{"id":"m1"},{"id":"m2"}]}')
    ).toBe('patch · 2 行')
    expect(taskBoardToolSummary('{"method":"init","board_len":5}')).toBe('共 5 里程碑')
    expect(taskBoardToolSummary('{"method":"patch"}')).toBe('任务板 · patch')
    expect(taskBoardToolSummary('{"summary":{"method":"prune"}}')).toBe(
      '任务板 · prune'
    )
  })

  it('keeps the fallback for malformed task board result text', () => {
    expect(taskBoardToolSummary('plain text')).toBe('任务板已更新')
  })

  it('derives patch summaries from arguments with item_id/status', () => {
    expect(
      taskBoardPatchSummaryFromArgs('{"item_id":"wi_3","status":"done"}')
    ).toBe('#wi_3 → done')
    expect(
      taskBoardPatchSummaryFromArgs('{"milestones":[{"id":"m2","status":"in_progress"}]}')
    ).toBeNull()
    expect(
      taskBoardPatchSummaryFromArgs('{"items":[{"id":"a","status":"done"},{"id":"b"}]}')
    ).toBe('更新 · 2 行')
  })

  it('filters sidecar tool calls by default but keeps them when enabled', () => {
    const calls: ToolCall[] = [
      { id: '1', name: 'ask_user', status: 'success', arguments: '{}', result: '{}' },
      { id: '2', name: 'task_board_patch', status: 'success', arguments: '{}', result: '{}' }
    ]
    expect(visibleToolCalls(calls).map(c => c.name)).toEqual(['ask_user'])
    expect(visibleToolCalls(calls, [], true).map(c => c.name)).toEqual([
      'ask_user',
      'task_board_patch'
    ])
  })

  it('hides task_board_init in a mixed round unless sidecar is enabled', () => {
    const calls: ToolCall[] = [
      { id: '1', name: 'file_read', status: 'success', arguments: '{}', result: '{}' },
      { id: '2', name: 'task_board_init', status: 'success', arguments: '{}', result: '{}' }
    ]
    expect(visibleToolCalls(calls).map(c => c.name)).toEqual(['file_read'])
    expect(visibleToolCalls(calls, [], true).map(c => c.name)).toEqual([
      'file_read',
      'task_board_init'
    ])
  })
})
