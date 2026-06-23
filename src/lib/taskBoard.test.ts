import { describe, expect, it } from 'vitest'
import type { TaskBoardDocument } from '../types/chat'
import {
  hasTaskBoardContent,
  taskBoardGlobalMilestones,
  taskBoardHasWorkItems,
  taskBoardItemMilestones
} from './taskBoard'

function v4Doc(): TaskBoardDocument {
  return {
    version: 4,
    task_id: 'tb_test',
    meta: {
      goal: '批量添加地址',
      status: 'running',
      work_item_mode: 'dynamic',
      expected_total: 127
    },
    global_milestones: [
      { id: 'g_plan', title: '计划', status: 'done' },
      { id: 'g_exec', title: '执行', status: 'ready' },
      { id: 'g_deliver', title: '交付', status: 'ready' }
    ],
    item_milestones: [
      { id: 'm1', title: '添加工作地址', status: 'pending' }
    ]
  }
}

describe('taskBoard helpers', () => {
  it('reads v4 global_milestones', () => {
    const rows = taskBoardGlobalMilestones(v4Doc())
    expect(rows).toHaveLength(3)
    expect(rows[0]?.id).toBe('g_plan')
  })

  it('falls back to legacy board', () => {
    const doc: TaskBoardDocument = {
      version: 3,
      task_id: 'tb_legacy',
      meta: { goal: 'g', status: 'running' },
      board: [{ id: 'm1', title: 'step', status: 'pending' }]
    }
    expect(taskBoardGlobalMilestones(doc)).toHaveLength(1)
  })

  it('prefers global_milestones over legacy board', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_mix',
      meta: { goal: 'g', status: 'running' },
      global_milestones: [{ id: 'g1', title: 'v4', status: 'pending' }],
      board: [{ id: 'b1', title: 'v3', status: 'done' }]
    }
    expect(taskBoardGlobalMilestones(doc)[0]?.id).toBe('g1')
  })

  it('detects work items from meta', () => {
    expect(taskBoardHasWorkItems(v4Doc())).toBe(true)
    expect(taskBoardItemMilestones(v4Doc())).toHaveLength(1)
    expect(hasTaskBoardContent(v4Doc())).toBe(true)
  })
})
