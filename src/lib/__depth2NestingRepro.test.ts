import { describe, expect, it } from 'vitest'
import type { AgentTrace, ChatMessage, ToolCall } from '../types/chat'
import { buildTurnSpawnPlan, framesForMessage } from './subAgentRenderTree'
import { isScopedSubMessage } from './subAgentMessages'

/**
 * 真实 case 夹具：会话 `c7b8306b-3026-41d7-889d-c434dd74602f`（差旅费报销第 3 批测试），
 * 行 `position` 43756-43770 的 payload 原样搬运（字段名 / 取值一字未改）。
 *
 * 结构：lead 消息 → depth-1 scoped 行（coder，自己带 3 个 run_subagent 调用）→ 3 个 depth-2
 * explore 行组。depth-1 与 depth-2 的行在 position 上**交错**，所以任何"连续区间归组"都错。
 */
const CONV = 'c7b8306b-3026-41d7-889d-c434dd74602f'
const LEAD_ID = 'msg_23835474f80a4d8fab1f630a990c97c7'
const PARENT_ID = 'agent_msg_49fe2b5d374f4f8e8c7ad4eeb62a4801'
const LEAD_SPAWN_CALL = 'call_00_3sIXVuLiNd1BCJbTGSkG2731'
const PARENT_INSTANCE = '8ea61fb4-71d6-4cc3-b3c6-ee3f15c921ec'

/** 父行的 3 个 run_subagent 调用（position 43756 的 payload.toolCalls）。 */
const parentCalls: ToolCall[] = [
  { id: 'call_00_1wgTxRryGTHv8xilTSLB0605', name: 'run_subagent', status: 'success', arguments: '{"task":"D1/D2 凭证归属与卡号丢失"}' },
  { id: 'call_01_0f5TYB8pVii5kmuKz93f4234', name: 'run_subagent', status: 'success', arguments: '{"task":"D3/D4 组装层重复计入与科目错位"}' },
  { id: 'call_02_0xhcyQuJzaUxQELjhqBk2086', name: 'run_subagent', status: 'success', arguments: '{"task":"D5/D6 预览文案与发票去重"}' }
]

/** 父行的 agentTrace（position 43756 的 payload.agentTrace，3 条 depth=2，全 completed）。 */
function depthTwoTrace(id: string, callId: string, name: string): AgentTrace {
  return {
    id,
    name,
    role: 'worker',
    status: 'completed',
    detail: '## Summary',
    content: undefined, // DB 里是 null；应用类型是 string | undefined
    depth: 2,
    collapsed: true,
    userExpanded: false,
    agentInstanceId: id,
    parentToolCallId: callId,
    anchorMessageId: PARENT_ID,
    taskId: callId,
    agentId: 'explore',
    parentTraceId: PARENT_INSTANCE,
    delegation: 'registered'
  } as AgentTrace
}

const traces: AgentTrace[] = [
  depthTwoTrace('04a1d107-81c2-4f31-9da4-d01c780bafc4', 'call_00_1wgTxRryGTHv8xilTSLB0605', '代码探索'),
  depthTwoTrace('95f07c33-ad3d-41d1-8deb-fa93f9ca318a', 'call_01_0f5TYB8pVii5kmuKz93f4234', '代码探索'),
  depthTwoTrace('773b5e29-ecb1-43e2-8846-408bbc81bf72', 'call_02_0xhcyQuJzaUxQELjhqBk2086', '代码探索')
]

/** lead 消息：agentTrace 为空、run_subagent 调用仍是 running（payload 原样）。 */
const lead: ChatMessage = {
  id: LEAD_ID,
  role: 'assistant',
  content: 'This is a cross-layer mapping.',
  status: 'streaming',
  createdAt: 1,
  toolCalls: [
    { id: LEAD_SPAWN_CALL, name: 'run_subagent', status: 'running', arguments: '{"agent":"coder"}' }
  ]
} as ChatMessage

/** depth-1 scoped 行：自己带 3 个 run_subagent 调用 + 3 条 depth=2 档案。 */
const parentRow: ChatMessage = {
  id: PARENT_ID,
  role: 'assistant',
  content: 'While the workers map the stages…',
  status: 'streaming',
  createdAt: 2,
  traceId: 'sub_task_1e857c70a88f475e91dfac461aecbc3d:coder',
  agentInstanceId: PARENT_INSTANCE,
  anchorMessageId: LEAD_ID,
  spawnDepth: 1,
  toolCalls: parentCalls,
  agentTrace: traces
} as ChatMessage

/** depth-2 子行：traceId = `{callId}:{instance}:{agentId}`，anchor = 父行自己。 */
function childRow(position: number, role: 'user' | 'assistant' | 'tool', callId: string, instance: string, toolCalls?: ToolCall[]): ChatMessage {
  return {
    id: `row_${position}`,
    role,
    content: role === 'user' ? 'Begin. Your assigned task' : '',
    status: 'done',
    createdAt: position,
    traceId: `${callId}:${instance}:explore`,
    agentInstanceId: instance,
    anchorMessageId: PARENT_ID,
    spawnDepth: 2,
    toolCalls
  } as ChatMessage
}

const childRows: ChatMessage[] = [
  childRow(43759, 'user', 'call_00_1wgTxRryGTHv8xilTSLB0605', '04a1d107-81c2-4f31-9da4-d01c780bafc4'),
  childRow(43761, 'assistant', 'call_00_1wgTxRryGTHv8xilTSLB0605', '04a1d107-81c2-4f31-9da4-d01c780bafc4'),
  childRow(43763, 'user', 'call_01_0f5TYB8pVii5kmuKz93f4234', '95f07c33-ad3d-41d1-8deb-fa93f9ca318a'),
  childRow(43764, 'assistant', 'call_01_0f5TYB8pVii5kmuKz93f4234', '95f07c33-ad3d-41d1-8deb-fa93f9ca318a'),
  childRow(43765, 'user', 'call_02_0xhcyQuJzaUxQELjhqBk2086', '773b5e29-ecb1-43e2-8846-408bbc81bf72'),
  childRow(43766, 'assistant', 'call_02_0xhcyQuJzaUxQELjhqBk2086', '773b5e29-ecb1-43e2-8846-408bbc81bf72'),
  childRow(43767, 'tool', 'call_00_1wgTxRryGTHv8xilTSLB0605', '04a1d107-81c2-4f31-9da4-d01c780bafc4', [
    { id: 'tc_file_list_00', name: 'file_list', status: 'success', arguments: '{"directory":"/Users/sta"}' }
  ]),
  childRow(43768, 'tool', 'call_00_1wgTxRryGTHv8xilTSLB0605', '04a1d107-81c2-4f31-9da4-d01c780bafc4', [
    { id: 'tc_grep_00', name: 'file_grep', status: 'success', arguments: '{"root":"/Users/starliu/"}' }
  ]),
  childRow(43770, 'assistant', 'call_00_1wgTxRryGTHv8xilTSLB0605', '04a1d107-81c2-4f31-9da4-d01c780bafc4')
]

describe('depth-2 嵌套（真实 case）', () => {
  it('三个 depth-2 子帧各自挂到父行的 run_subagent 行上，不进孤儿组', () => {
    // 视图侧只报**回合级消息**画出来的行（MessageList 的 drawnRowIdsByMessage 就是这么建的）。
    const plan = buildTurnSpawnPlan({
      messages: [lead],
      scopedRows: [parentRow, ...childRows],
      drawnRowIdsByMessage: new Map([[lead.id, [LEAD_SPAWN_CALL]]])
    })

    const frames = framesForMessage(plan, parentRow)

    // 复现：父行的 3 条工具行是「帧内行」，视图没把它们算进 drawn。
    const structure = {
      byRow: [...frames.byRow.keys()],
      unplaced: frames.unplaced.map(t => t.id),
      unbound: frames.unbound.map(t => t.id)
    }
    // eslint-disable-next-line no-console
    console.log('[repro] framesForMessage(parentRow) =', JSON.stringify(structure, null, 2))

    expect(frames.byRow.size).toBe(3)
    expect(frames.unplaced).toEqual([])
    expect(frames.unbound).toEqual([])
  })

  it('子行按真实 traceId / agentInstanceId 能被父级查到（数据层归属链）', () => {
    expect(parentRow.traceId).toBe('sub_task_1e857c70a88f475e91dfac461aecbc3d:coder')
    expect(childRows.every(r => isScopedSubMessage(r))).toBe(true)
    expect(childRows.every(r => r.anchorMessageId === PARENT_ID)).toBe(true)
    expect(new Set(childRows.map(r => r.agentInstanceId)).size).toBe(3)
    // 三条档案的宿主行 = 父行的三条 run_subagent 调用，一一对应。
    expect(traces.map(t => t.parentToolCallId)).toEqual(parentCalls.map(tc => tc.id))
    void CONV
  })
})
