import { computed } from 'vue'
import { describe, expect, it } from 'vitest'
import type { AgentTrace, ChatMessage } from '../types/chat'
import { createConversationScopedStore } from './conversationScoped/store'
import { buildTurnSpawnPlan, framesForMessage } from './subAgentRenderTree'
import { isSubTraceUiCollapsed, subTraceUiSignal, toggleSubTraceExpanded } from './subAgentSession'

/**
 * 「嵌套子 agent 帧点击后不展开」的复现夹具。
 *
 * 形状取自真实数据（会话 c7b8306b…，行 43756 的 payload）：父 worker 的 scoped 行
 * 自己带 `run_subagent` 调用，depth-2 的档案挂在**它的** `agentTrace` 上，
 * `parentToolCallId` 指回父行的调用。截图里的 `氛围编程（fork）· 搜索 1 次 · 读文件 22 次`
 * 就是这个位置：depth-2 帧由 `SubAgentFrame` 内的 `SubAgentFrameHost` 渲染。
 */
const CONV = 'c1'
const LEAD_ID = 'msg_lead'
const PARENT_ID = 'agent_msg_parent'
const PARENT_INSTANCE = '8ea61fb4-71d6-4cc3-b3c6-ee3f15c921ec'
const PARENT_CALL = 'call_00_1wgTxRryGTHv8xilTSLB0605'
const CHILD_INSTANCE = '04a1d107-81c2-4f31-9da4-d01c780bafc4'

const childTrace: AgentTrace = {
  id: CHILD_INSTANCE,
  name: '氛围编程',
  role: 'worker',
  status: 'running',
  depth: 2,
  collapsed: true,
  userExpanded: false,
  agentInstanceId: CHILD_INSTANCE,
  parentToolCallId: PARENT_CALL,
  anchorMessageId: PARENT_ID,
  taskId: PARENT_CALL,
  agentId: 'coder',
  parentTraceId: PARENT_INSTANCE,
  delegation: 'registered'
} as AgentTrace

const lead: ChatMessage = {
  id: LEAD_ID,
  role: 'assistant',
  content: 'lead',
  status: 'streaming',
  createdAt: 1
} as ChatMessage

const parentRow: ChatMessage = {
  id: PARENT_ID,
  role: 'assistant',
  content: 'parent worker',
  status: 'streaming',
  createdAt: 2,
  traceId: 'sub_task_x:coder',
  agentInstanceId: PARENT_INSTANCE,
  anchorMessageId: LEAD_ID,
  spawnDepth: 1,
  toolCalls: [
    { id: PARENT_CALL, name: 'run_subagent', status: 'running', arguments: '{"agentId":"coder"}' }
  ],
  agentTrace: [childTrace]
} as ChatMessage

const parentLookup = {
  agentInstanceId: PARENT_INSTANCE,
  anchorMessageId: LEAD_ID,
  traceId: 'sub_task_x:coder'
}

describe('嵌套帧点击展开（复现）', () => {
  it('帧拿到的 trace 就是 scoped store 里那一个对象（身份）', () => {
    const store = createConversationScopedStore()
    store.ensureInstance(CONV, parentLookup, parentRow)
    const rows = store.getRows(CONV, parentLookup)
    expect(rows[0]).toBe(parentRow)

    // 渲染管线（帧经 framesForMessage → node.trace 拿到的引用）
    const plan = buildTurnSpawnPlan({
      messages: [lead],
      scopedRows: rows,
      drawnRowIdsByMessage: new Map([[LEAD_ID, []]])
    })
    const frames = framesForMessage(plan, parentRow)
    const fromTree = frames.byRow.get(PARENT_CALL)?.[0]
    // 身份一致：点击翻的就是这一条（不是派生副本）
    expect(fromTree).toBe(rows[0]!.agentTrace![0])
  })

  it('点击后 userExpanded 翻了，视图必须能观察到（否则永远不展开）', () => {
    const store = createConversationScopedStore()
    store.ensureInstance(CONV, parentLookup, parentRow)
    const trace = store.getRows(CONV, parentLookup)[0]!.agentTrace![0]!

    // 组件里的写法（修复后）：先读一次 UI revision，再读标志
    const revisionBefore = subTraceUiSignal()
    const collapsed = computed(() => {
      void subTraceUiSignal()
      return isSubTraceUiCollapsed(trace)
    })
    expect(collapsed.value).toBe(true)

    // 点击：CollapsedRunHeader @toggle → SubAgentFrame.toggleExpanded()
    toggleSubTraceExpanded(trace)
    expect(trace.userExpanded).toBe(true)
    expect(trace.collapsed).toBe(false)
    // 契约：翻转必须同时推进 revision —— 否则读标志的 computed 永远不会重算
    expect(subTraceUiSignal()).toBeGreaterThan(revisionBefore)
    // 视图现在看得见这次翻转 → 帧会展开
    expect(collapsed.value).toBe(false)
  })

  it('展开后帧能画出它自己的行（当前代码下已成立）', () => {
    const store = createConversationScopedStore()
    store.ensureInstance(CONV, parentLookup, parentRow)
    const trace = store.getRows(CONV, parentLookup)[0]!.agentTrace![0]!

    // depth-2 子行：anchor = 父行、traceId = `call:instance:agent`、instance = 档案自己
    const childRow: ChatMessage = {
      id: 'row_child',
      role: 'tool',
      content: '{"root":"/Users/starliu/"}',
      status: 'done',
      createdAt: 3,
      traceId: `${PARENT_CALL}:${CHILD_INSTANCE}:coder`,
      agentInstanceId: CHILD_INSTANCE,
      anchorMessageId: PARENT_ID,
      spawnDepth: 2,
      toolCalls: [{ id: 'tc_child', name: 'file_grep', status: 'success', arguments: '{}' }]
    } as ChatMessage
    store.ensureInstance(CONV, {
      agentInstanceId: CHILD_INSTANCE,
      anchorMessageId: PARENT_ID,
      traceId: `${PARENT_CALL}:${CHILD_INSTANCE}:coder`
    }, childRow)

    // 帧取自己那几行的口径（与 SubAgentFrameHost 一致）
    const own = store.getRows(CONV, {
      anchorMessageId: trace.anchorMessageId,
      traceId: trace.id,
      agentInstanceId: trace.agentInstanceId
    })
    expect(own.map(r => r.id)).toEqual(['row_child'])
  })
})
