import { describe, expect, it } from 'vitest'
import type { AgentTrace, ChatMessage, ToolCall } from '../types/chat'
import {
  buildTurnSpawnPlan,
  buildTurnSpawnTree,
  framesForMessage,
  missingHostFetches,
  SPAWN_TOOL_BASE_NAME
} from './subAgentRenderTree'

function spawnRow(id: string): ToolCall {
  return { id, name: SPAWN_TOOL_BASE_NAME, arguments: '{}', status: 'success' } as unknown as ToolCall
}

function jobRow(id: string): ToolCall {
  return {
    id,
    name: 'job',
    arguments: '{"action":"await","mode":"all"}',
    status: 'running'
  } as unknown as ToolCall
}

function trace(overrides: Partial<AgentTrace> & { id: string }): AgentTrace {
  return {
    name: 'Explore',
    role: 'worker',
    status: 'completed',
    depth: 1,
    ...overrides
  }
}

function leadMessage(overrides: Partial<ChatMessage> & { id: string }): ChatMessage {
  return { role: 'assistant', content: '', status: 'done', createdAt: 1, ...overrides }
}

function scopedRow(
  overrides: Partial<ChatMessage> & { id: string; agentInstanceId: string }
): ChatMessage {
  return {
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 2,
    anchorMessageId: 'a1',
    ...overrides
  }
}

/** Two background writers: rows on one round, traces on the next one. */
function twoWritersAcrossRounds() {
  const dispatch = leadMessage({
    id: 'a1',
    toolCalls: [spawnRow('r1'), spawnRow('r2')]
  })
  const awaiting = leadMessage({
    id: 'a2',
    toolCalls: [jobRow('r_await')],
    agentTrace: [
      trace({ id: 'inst_1', parentToolCallId: 'r1' }),
      trace({ id: 'inst_2', parentToolCallId: 'r2' })
    ]
  })
  return buildTurnSpawnTree({ messages: [dispatch, awaiting] })
}

describe('buildTurnSpawnTree · I1 ownership is data', () => {
  it('resolves a host row that lives on a different round than its trace', () => {
    const tree = twoWritersAcrossRounds()
    expect(tree.nodeForRow('r1')?.trace?.id).toBe('inst_1')
    expect(tree.nodeForRow('r2')?.trace?.id).toBe('inst_2')
    expect(tree.unbound).toHaveLength(0)
  })

  it('resolves a host row carried by a trailing tool group', () => {
    const tree = buildTurnSpawnTree({
      messages: [leadMessage({ id: 'a2', toolCalls: [jobRow('r_await')] })],
      extraToolCallsByMessageId: new Map([['a1', [spawnRow('r1')]]]),
      scopedRows: [
        scopedRow({
          id: 's1',
          agentInstanceId: 'inst_1',
          agentTrace: [trace({ id: 'inst_1', parentToolCallId: 'r1' })]
        })
      ]
    })
    expect(tree.nodeForRow('r1')?.trace?.id).toBe('inst_1')
    expect(tree.unbound).toHaveLength(0)
  })

  it('never treats a job row as a parent', () => {
    const tree = buildTurnSpawnTree({
      messages: [
        leadMessage({
          id: 'a1',
          toolCalls: [jobRow('r_await')],
          agentTrace: [trace({ id: 'inst_1', parentToolCallId: 'r_await' })]
        })
      ]
    })
    expect(tree.nodeForRow('r_await')).toBeNull()
    expect(tree.unbound.map(node => node.trace?.id)).toEqual(['inst_1'])
  })

  it('keeps a trace without a host row in the turn visible as unbound', () => {
    const tree = buildTurnSpawnTree({
      messages: [leadMessage({ id: 'a1', agentTrace: [trace({ id: 'inst_legacy' })] })]
    })
    expect(tree.unbound.map(node => node.trace?.id)).toEqual(['inst_legacy'])
    expect(tree.nodeForTrace('inst_legacy')?.unbound).toBe(true)
    expect(tree.nodeForTrace('inst_legacy')?.rowId).toBe('')
  })

  it('claims a host row once and reports the duplicate claim', () => {
    const tree = buildTurnSpawnTree({
      messages: [
        leadMessage({
          id: 'a1',
          toolCalls: [spawnRow('r1')],
          agentTrace: [
            trace({ id: 'inst_first', parentToolCallId: 'r1' }),
            trace({ id: 'inst_second', parentToolCallId: 'r1' })
          ]
        })
      ]
    })
    expect(tree.duplicatedHostRowIds).toEqual(['r1'])
    expect(tree.nodeForRow('r1')?.trace?.id).toBe('inst_first')
    expect(tree.unbound.map(node => node.trace?.id)).toEqual(['inst_second'])
  })

  it('reports spawn rows that have no trace yet', () => {
    const tree = buildTurnSpawnTree({
      messages: [leadMessage({ id: 'a1', toolCalls: [spawnRow('r1')] })]
    })
    expect(tree.pendingRows.map(ref => ref.rowId)).toEqual(['r1'])
    expect(tree.pendingRows[0]?.messageId).toBe('a1')
    expect(tree.nodeForRow('r1')).toBeNull()
  })
})

describe('buildTurnSpawnTree · hierarchy', () => {
  it('nests a grandchild under its parent node', () => {
    const tree = buildTurnSpawnTree({
      messages: [
        leadMessage({
          id: 'a1',
          toolCalls: [spawnRow('r_parent')],
          agentTrace: [trace({ id: 'inst_parent', parentToolCallId: 'r_parent' })]
        })
      ],
      scopedRows: [
        scopedRow({
          id: 's1',
          agentInstanceId: 'inst_parent',
          toolCalls: [spawnRow('r_child')],
          agentTrace: [
            trace({
              id: 'inst_child',
              depth: 2,
              parentTraceId: 'inst_parent',
              parentToolCallId: 'r_child'
            })
          ]
        })
      ]
    })
    expect(tree.childrenOfRow('r_parent').map(node => node.trace?.id)).toEqual(['inst_child'])
    expect(tree.childrenOfRow('r_child')).toEqual([])
    expect(tree.roots.map(node => node.trace?.id)).toEqual(['inst_parent'])
  })
})

describe('buildTurnSpawnTree · I2 collapse only decides what is drawn', () => {
  it('places each frame on its own row surface when the rows are drawn', () => {
    const plan = twoWritersAcrossRounds().planSurfaces({ drawn: new Set(['r1', 'r2']) })
    expect(plan.framesBySurface.get('r1')?.map(node => node.rowId)).toEqual(['r1'])
    expect(plan.framesBySurface.get('r2')?.map(node => node.rowId)).toEqual(['r2'])
    expect([...plan.lifted]).toEqual([])
    expect(plan.detached).toEqual([])
  })

  it('lifts a hidden row frame onto the drawn container', () => {
    const plan = twoWritersAcrossRounds().planSurfaces({
      drawn: new Set(['group:collapsed', 'r1']),
      containerOf: key => (key === 'r2' ? 'group:collapsed' : key)
    })
    expect(plan.framesBySurface.get('r1')?.map(node => node.rowId)).toEqual(['r1'])
    expect(plan.framesBySurface.get('group:collapsed')?.map(node => node.rowId)).toEqual(['r2'])
    expect([...plan.lifted]).toEqual(['row:r2'])
    expect(plan.detached).toEqual([])
  })

  it('reports detached instead of dropping when no drawn container exists', () => {
    const plan = twoWritersAcrossRounds().planSurfaces({
      drawn: new Set(['r1']),
      containerOf: key => (key === 'r2' ? 'group:collapsed' : key)
    })
    expect(plan.framesBySurface.get('group:collapsed')).toBeUndefined()
    expect(plan.detached.map(node => node.rowId)).toEqual(['r2'])
  })
})

describe('buildTurnSpawnTree · concurrent background writers', () => {
  it('keeps one frame per writer under its own row, whatever round carried it', () => {
    // Shape of the reported incident: N background spawns spread over three rounds,
    // traces materialised on the round that awaits them.
    const rounds = [
      leadMessage({ id: 'a1', toolCalls: [spawnRow('r1'), spawnRow('r2'), spawnRow('r3')] }),
      leadMessage({ id: 'a2', toolCalls: [spawnRow('r4')] }),
      leadMessage({ id: 'a3', toolCalls: [spawnRow('r5')] }),
      leadMessage({
        id: 'a4',
        toolCalls: [jobRow('r_await')],
        agentTrace: [1, 2, 3, 4, 5].map(index =>
          trace({
            id: `inst_${index}`,
            parentToolCallId: `r${index}`,
            status: index === 5 ? 'running' : 'completed'
          })
        )
      })
    ]
    const tree = buildTurnSpawnTree({ messages: rounds })
    expect(tree.nodes).toHaveLength(5)
    expect(tree.unbound).toHaveLength(0)
    expect(tree.pendingRows).toHaveLength(0)

    const plan = tree.planSurfaces({ drawn: new Set(['r1', 'r2', 'r3', 'r4', 'r5']) })
    expect([...plan.framesBySurface.keys()]).toEqual(['r1', 'r2', 'r3', 'r4', 'r5'])
    expect(plan.framesBySurface.get('r5')?.[0]?.trace?.status).toBe('running')
    expect(plan.detached).toEqual([])
  })
})

describe('buildTurnSpawnPlan · 回合级归属（P1）', () => {
  /** 两个回合条目：工单行在第 1 条消息，档案被投递到第 2 条消息（后台派发的形状）。 */
  function twoRoundPlan(drawnA1: string[], drawnA2: string[]) {
    const dispatch = leadMessage({ id: 'a1', toolCalls: [spawnRow('r1'), spawnRow('r2')] })
    const awaiting = leadMessage({
      id: 'a2',
      toolCalls: [jobRow('r_await')],
      agentTrace: [
        trace({ id: 'inst_1', parentToolCallId: 'r1' }),
        trace({ id: 'inst_2', parentToolCallId: 'r2' })
      ]
    })
    return {
      dispatch,
      awaiting,
      plan: buildTurnSpawnPlan({
        messages: [dispatch, awaiting],
        drawnRowIdsByMessage: new Map([
          ['a1', drawnA1],
          ['a2', drawnA2]
        ])
      })
    }
  }

  it('把跨轮投递的档案挂到它自己的工单行下', () => {
    const { dispatch, awaiting, plan } = twoRoundPlan(['r1', 'r2'], ['r_await'])
    expect(framesForMessage(plan, dispatch).byRow.get('r1')?.map(t => t.id)).toEqual(['inst_1'])
    expect(framesForMessage(plan, dispatch).byRow.get('r2')?.map(t => t.id)).toEqual(['inst_2'])
    // 携带档案的那条消息不再自己画（宿主行已在别处画出），也不进兜底 —— 不重复
    const atAwaiting = framesForMessage(plan, awaiting)
    expect(atAwaiting.byRow.size).toBe(0)
    expect(atAwaiting.unplaced).toEqual([])
    expect(atAwaiting.unbound).toEqual([])
  })

  it('宿主行被折叠隐藏时进未关联组，且只出现一次', () => {
    const { dispatch, awaiting, plan } = twoRoundPlan(['r1'], [])
    expect(framesForMessage(plan, dispatch).byRow.get('r1')?.map(t => t.id)).toEqual(['inst_1'])
    expect(framesForMessage(plan, dispatch).byRow.has('r2')).toBe(false)
    const atAwaiting = framesForMessage(plan, awaiting)
    expect(atAwaiting.unplaced.map(t => t.id)).toEqual(['inst_2'])
    expect(atAwaiting.unbound).toEqual([])
  })

  it('宿主行在本回合不存在 → unbound；job 行不算宿主', () => {
    const message = leadMessage({
      id: 'a1',
      toolCalls: [jobRow('r_await')],
      agentTrace: [
        trace({ id: 'inst_job', parentToolCallId: 'r_await' }),
        trace({ id: 'inst_ghost', parentToolCallId: 'r_ghost' }),
        trace({ id: 'inst_none' })
      ]
    })
    const plan = buildTurnSpawnPlan({
      messages: [message],
      drawnRowIdsByMessage: new Map([['a1', ['r_await']]])
    })
    const frames = framesForMessage(plan, message)
    expect(frames.byRow.size).toBe(0)
    expect(frames.unplaced).toEqual([])
    expect(frames.unbound.map(t => t.id)).toEqual(['inst_job', 'inst_ghost', 'inst_none'])
  })

  it('场景 A（行与档案同一条消息）与旧行为一致', () => {
    const message = leadMessage({
      id: 'a1',
      toolCalls: [spawnRow('r1')],
      agentTrace: [trace({ id: 'inst_1', parentToolCallId: 'r1' })]
    })
    const plan = buildTurnSpawnPlan({
      messages: [message],
      drawnRowIdsByMessage: new Map([['a1', ['r1']]])
    })
    const frames = framesForMessage(plan, message)
    expect(frames.byRow.get('r1')?.map(t => t.id)).toEqual(['inst_1'])
    expect(frames.unplaced).toEqual([])
    expect(frames.unbound).toEqual([])
    expect([...plan.drawnRowIds]).toEqual(['r1'])
  })

  it('行来源覆盖 trailing group 与 scoped 行（嵌套派发行）', () => {
    const host = leadMessage({ id: 'a1', toolCalls: [jobRow('r_await')] })
    const plan = buildTurnSpawnPlan({
      messages: [host],
      extraToolCallsByMessageId: new Map([['a0', [spawnRow('r_trailing')]]]),
      scopedRows: [
        scopedRow({
          id: 's1',
          agentInstanceId: 'inst_parent',
          toolCalls: [spawnRow('r_nested')],
          agentTrace: [
            trace({
              id: 'inst_nested',
              depth: 2,
              parentTraceId: 'inst_parent',
              parentToolCallId: 'r_nested'
            })
          ]
        })
      ],
      drawnRowIdsByMessage: new Map([['a0', ['r_trailing']]])
    })
    // trailing group 的行被索引为「已派发、尚无档案」
    expect(plan.tree.nodeForRow('r_trailing')).toBeNull()
    expect(plan.tree.pendingRows.map(ref => ref.rowId)).toEqual(['r_trailing'])
    // 嵌套派发行（父 worker 的 scoped 行上）也能解析到自己的档案
    expect(plan.tree.nodeForRow('r_nested')?.trace?.id).toBe('inst_nested')
  })
})

describe('missingHostFetches · P0.1 缺行补拉', () => {
  /** 孙 agent：派发行写在父 worker 的 scoped 行上，而那条行**没有加载**。 */
  function nestedUnloaded() {
    const host = leadMessage({
      id: 'a1',
      toolCalls: [spawnRow('r_parent')],
      agentTrace: [trace({ id: 'inst_parent', parentToolCallId: 'r_parent' })]
    })
    const message = leadMessage({
      id: 'a2',
      agentTrace: [
        trace({
          id: 'inst_nested',
          depth: 2,
          parentTraceId: 'inst_parent',
          parentToolCallId: 'r_nested',
          anchorMessageId: 's_parent'
        })
      ]
    })
    return { message, plan: buildTurnSpawnPlan({ messages: [host, message] }) }
  }

  it('嵌套层宿主行未加载 → 给出补拉请求（锚点用档案自己的 anchorMessageId）', () => {
    const { message, plan } = nestedUnloaded()
    expect(missingHostFetches(plan, message)).toEqual([
      { traceId: 'inst_nested', anchorMessageId: 's_parent', agentInstanceId: undefined }
    ])
  })

  it('父 worker 的 scoped 行已在索引里 → 不再请求', () => {
    const host = leadMessage({
      id: 'a1',
      toolCalls: [spawnRow('r_parent')],
      agentTrace: [trace({ id: 'inst_parent', parentToolCallId: 'r_parent' })]
    })
    const parentRow = scopedRow({
      id: 's_parent',
      agentInstanceId: 'inst_parent',
      toolCalls: [spawnRow('r_nested')],
      agentTrace: [
        trace({
          id: 'inst_nested',
          depth: 2,
          parentTraceId: 'inst_parent',
          parentToolCallId: 'r_nested',
          anchorMessageId: 's_parent'
        })
      ]
    })
    const plan = buildTurnSpawnPlan({ messages: [host], scopedRows: [parentRow] })
    expect(missingHostFetches(plan, parentRow)).toEqual([])
  })

  it('已请求过的不再返回（每个 trace 只拉一次）', () => {
    const { message, plan } = nestedUnloaded()
    expect(missingHostFetches(plan, message, new Set(['inst_nested']))).toEqual([])
  })

  it('没有 parentToolCallId 的历史档案不补拉', () => {
    const message = leadMessage({ id: 'a1', agentTrace: [trace({ id: 'inst_legacy' })] })
    const plan = buildTurnSpawnPlan({ messages: [message] })
    expect(missingHostFetches(plan, message)).toEqual([])
  })

  it('锚点缺失时回落到本条消息 id', () => {
    const message = leadMessage({
      id: 'a1',
      agentTrace: [trace({ id: 'inst_x', parentToolCallId: 'r_missing' })]
    })
    const plan = buildTurnSpawnPlan({ messages: [message] })
    expect(missingHostFetches(plan, message)).toEqual([
      { traceId: 'inst_x', anchorMessageId: 'a1', agentInstanceId: undefined }
    ])
  })
})
