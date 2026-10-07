import { describe, expect, it } from 'vitest'
import type { AgentTrace, ChatMessage, ToolCall } from '../types/chat'
import {
  buildTurnSpawnPlan,
  buildTurnSpawnTree,
  framesForLayer,
  framesForMessage,
  missingHostFetches,
  scopedRowClosure,
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

describe('missingHostFetches · P0.1 缺行补拉（父级）', () => {
  /**
   * 父（第 1 层，锚在 a1）派了子；子的档案被投递到 lead 的另一条消息上（投递漂移），
   * 而父自己的行**还没加载** → 子的宿主行 `r_nested` 不在索引里。
   */
  function driftedNested(opts: { parentLoaded?: boolean } = {}) {
    const dispatch = leadMessage({
      id: 'a1',
      toolCalls: [spawnRow('r_parent')],
      agentTrace: [
        trace({ id: 'inst_parent', parentToolCallId: 'r_parent', anchorMessageId: 'a1' })
      ]
    })
    const drifted = leadMessage({
      id: 'a2',
      agentTrace: [
        trace({
          id: 'inst_nested',
          depth: 2,
          parentTraceId: 'inst_parent',
          parentToolCallId: 'r_nested',
          anchorMessageId: 'r_parent'
        })
      ]
    })
    const parentRow = scopedRow({
      id: 's_parent_row',
      agentInstanceId: 'inst_parent',
      toolCalls: [spawnRow('r_nested')]
    })
    const plan = buildTurnSpawnPlan({
      messages: [dispatch, drifted],
      scopedRows: opts.parentLoaded ? [parentRow] : []
    })
    return { dispatch, drifted, plan }
  }

  it('缺的是父级的行 → 请求指向父实例（不是档案自己）', () => {
    const { drifted, plan } = driftedNested()
    expect(missingHostFetches(plan, drifted)).toEqual([
      {
        key: 'a1\u0000inst_parent',
        traceId: 'inst_parent',
        anchorMessageId: 'a1',
        agentInstanceId: 'inst_parent',
        traceIds: ['inst_nested']
      }
    ])
  })

  it('父级的行已加载 → 不请求', () => {
    const { drifted, plan } = driftedNested({ parentLoaded: true })
    expect(missingHostFetches(plan, drifted)).toEqual([])
  })

  it('父 trace 不在本回合 → 不请求（拉自己那一份解析不出宿主行）', () => {
    const message = leadMessage({
      id: 'a1',
      agentTrace: [
        trace({
          id: 'inst_orphan',
          depth: 2,
          parentTraceId: 'inst_absent',
          parentToolCallId: 'r_missing'
        })
      ]
    })
    const plan = buildTurnSpawnPlan({ messages: [message] })
    expect(missingHostFetches(plan, message)).toEqual([])
  })

  it('同一个父实例下的多个档案合并成一个请求', () => {
    const dispatch = leadMessage({
      id: 'a1',
      toolCalls: [spawnRow('r_parent')],
      agentTrace: [
        trace({ id: 'inst_parent', parentToolCallId: 'r_parent', anchorMessageId: 'a1' })
      ]
    })
    const drifted = leadMessage({
      id: 'a2',
      agentTrace: [
        trace({ id: 'inst_a', depth: 2, parentTraceId: 'inst_parent', parentToolCallId: 'r_a' }),
        trace({ id: 'inst_b', depth: 2, parentTraceId: 'inst_parent', parentToolCallId: 'r_b' })
      ]
    })
    const plan = buildTurnSpawnPlan({ messages: [dispatch, drifted] })
    const fetches = missingHostFetches(plan, drifted)
    expect(fetches).toHaveLength(1)
    expect(fetches[0].traceIds).toEqual(['inst_a', 'inst_b'])
  })

  it('已请求过（同键）不再返回', () => {
    const { drifted, plan } = driftedNested()
    expect(missingHostFetches(plan, drifted, new Set(['a1\u0000inst_parent']))).toEqual([])
  })

  it('没有 parentToolCallId 的历史档案不补拉', () => {
    const message = leadMessage({ id: 'a1', agentTrace: [trace({ id: 'inst_legacy' })] })
    const plan = buildTurnSpawnPlan({ messages: [message] })
    expect(missingHostFetches(plan, message)).toEqual([])
  })
})

describe('scopedRowClosure · 嵌套行闭包', () => {
  it('把「锚在父级行上」的嵌套行也收进来', () => {
    const byAnchor = new Map<string, ChatMessage[]>([
      ['a1', [scopedRow({ id: 's1', agentInstanceId: 'inst_1' })]],
      ['s1', [scopedRow({ id: 's2', agentInstanceId: 'inst_2' })]]
    ])
    const rows = scopedRowClosure(['a1'], ids => ids.flatMap(id => byAnchor.get(id) ?? []))
    expect(rows.map(row => row.id)).toEqual(['s1', 's2'])
  })

  it('遵守 maxDepth 且不重复收集', () => {
    const byAnchor = new Map<string, ChatMessage[]>([
      ['a1', [scopedRow({ id: 's1', agentInstanceId: 'inst_1' })]],
      ['s1', [scopedRow({ id: 's2', agentInstanceId: 'inst_2' })]],
      ['s2', [scopedRow({ id: 's3', agentInstanceId: 'inst_3' })]]
    ])
    const collect = (ids: readonly string[]) => ids.flatMap(id => byAnchor.get(id) ?? [])
    expect(scopedRowClosure(['a1'], collect, 1).map(row => row.id)).toEqual(['s1'])
    expect(scopedRowClosure(['a1'], collect, 4).map(row => row.id)).toEqual(['s1', 's2', 's3'])
  })
})

describe('framesForLayer · 帧内挂载（P3）', () => {
  const parentTrace = () => trace({ id: 'inst_parent', parentToolCallId: 'r_head' })
  const childOf = (id: string, rowId: string) =>
    trace({ id, depth: 2, parentTraceId: 'inst_parent', parentToolCallId: rowId })

  it('子帧挂到它自己的工单行下，不画的行进兜底', () => {
    const childA = childOf('inst_a', 'r_a')
    const childB = childOf('inst_b', 'r_b')
    const parent = scopedRow({
      id: 'p1',
      agentInstanceId: 'inst_parent',
      toolCalls: [spawnRow('r_a'), spawnRow('r_b')],
      agentTrace: [childA, childB]
    })
    const plan = buildTurnSpawnPlan({
      messages: [parent],
      scopedRows: [parent],
      seedTraces: [parentTrace()],
      drawnRowIdsByMessage: new Map([['p1', ['r_a']]])
    })
    const frames = framesForLayer(plan, [childA, childB])
    expect(frames.byRow.get('r_a')?.map(t => t.id)).toEqual(['inst_a'])
    expect(frames.byRow.has('r_b')).toBe(false)
    expect(frames.detached.map(t => t.id)).toEqual(['inst_b'])
  })

  it('同一档案被重复投递到多条行上时只画一次', () => {
    const childA = childOf('inst_a', 'r_a')
    const first = scopedRow({
      id: 'p1',
      agentInstanceId: 'inst_parent',
      toolCalls: [spawnRow('r_a')],
      agentTrace: [childA]
    })
    const second = scopedRow({
      id: 'p2',
      agentInstanceId: 'inst_parent',
      toolCalls: [spawnRow('r_c')],
      agentTrace: [{ ...childA }]
    })
    const plan = buildTurnSpawnPlan({
      messages: [first, second],
      scopedRows: [first, second],
      seedTraces: [parentTrace()],
      drawnRowIdsByMessage: new Map([
        ['p1', ['r_a']],
        ['p2', ['r_c']]
      ])
    })
    const frames = framesForLayer(plan, [childA, { ...childA }])
    expect(frames.byRow.get('r_a')?.map(t => t.id)).toEqual(['inst_a'])
    expect(frames.byRow.has('r_c')).toBe(false)
    // 重复的那份不是「没画出来」，而是同一张行被认领两次 → 不再单独画
    expect(frames.detached).toEqual([])
  })

  it('帧自己的 trace 即使被 seed 也不会被画在自己层里（防自引用）', () => {
    const head = parentTrace()
    const childA = childOf('inst_a', 'r_a')
    const parent = scopedRow({
      id: 'p1',
      agentInstanceId: 'inst_parent',
      // 数据异常时 head 的宿主行也可能出现在本层行里 —— 仍然不能画
      toolCalls: [spawnRow('r_head'), spawnRow('r_a')],
      agentTrace: [childA]
    })
    const plan = buildTurnSpawnPlan({
      messages: [parent],
      scopedRows: [parent],
      seedTraces: [head],
      drawnRowIdsByMessage: new Map([['p1', ['r_head', 'r_a']]])
    })
    const frames = framesForLayer(plan, [childA])
    expect(frames.byRow.has('r_head')).toBe(false)
    expect(frames.byRow.get('r_a')?.map(t => t.id)).toEqual(['inst_a'])
    expect(frames.detached).toEqual([])
  })

  it('帧的行必须以 scopedRows 传进树（只传 messages 会整棵掉出树）', () => {
    const childA = childOf('inst_a', 'r_a')
    const parent = scopedRow({
      id: 'p1',
      agentInstanceId: 'inst_parent',
      toolCalls: [spawnRow('r_a')],
      agentTrace: [childA]
    })
    const plan = buildTurnSpawnPlan({
      messages: [parent],
      seedTraces: [parentTrace()],
      drawnRowIdsByMessage: new Map([['p1', ['r_a']]])
    })
    const frames = framesForLayer(plan, [childA])
    expect(frames.byRow.size).toBe(0)
    expect(frames.detached.map(t => t.id)).toEqual(['inst_a'])
  })

  it('本层没有画出来的行时全部进兜底', () => {
    const childA = childOf('inst_a', 'r_a')
    const parent = scopedRow({
      id: 'p1',
      agentInstanceId: 'inst_parent',
      toolCalls: [spawnRow('r_a')],
      agentTrace: [childA]
    })
    const plan = buildTurnSpawnPlan({
      messages: [parent],
      scopedRows: [parent],
      seedTraces: [parentTrace()]
    })
    const frames = framesForLayer(plan, [childA])
    expect(frames.byRow.size).toBe(0)
    expect(frames.detached.map(t => t.id)).toEqual(['inst_a'])
  })
})
