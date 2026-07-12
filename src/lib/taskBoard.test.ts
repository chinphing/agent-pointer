import { describe, expect, it } from 'vitest'
import type { TaskBoardDocument } from '../types/chat'
import {
  hasTaskBoardContent,
  taskBoardGlobalMilestones,
  taskBoardMilestoneViewMode,
  taskBoardIsLoopMilestoneBoard,
  taskBoardVisibleMilestoneProgress,
  taskBoardVisibleMilestones,
  taskBoardWorkItemsProgress
} from './taskBoard'

describe('taskBoard helpers', () => {
  it('reads v4 global_milestones', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_test',
      meta: { goal: 'Ship', status: 'running' },
      global_milestones: [
        { id: 'm1', title: 'Explore', status: 'pending' },
        { id: 'm2', title: 'Implement', status: 'pending' }
      ]
    }
    const rows = taskBoardGlobalMilestones(doc)
    expect(rows).toHaveLength(2)
    expect(rows[0]?.id).toBe('m1')
    expect(hasTaskBoardContent(doc)).toBe(true)
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

  it('linear board uses step mode', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_linear',
      meta: { goal: 'g', status: 'running' },
      global_milestones: [
        { id: 'm1', title: 'One', status: 'done' },
        { id: 'm2', title: 'Two', status: 'in_progress' }
      ]
    }
    expect(taskBoardMilestoneViewMode(doc)).toBe('step')
    expect(taskBoardVisibleMilestones(doc).map(r => r.id)).toEqual(['m1', 'm2'])
  })

  it('taskBoardWorkItemsProgress reads loop meta snapshot', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_loop',
      meta: {
        goal: 'Batch',
        status: 'running',
        work_items_done: 4,
        work_items_failed: 0,
        work_items_total: 10
      },
      global_milestones: [
        { id: 'g_plan', title: 'Plan', status: 'done' },
        { id: 'wi_1', title: '#1', status: 'done' },
        { id: 'g_deliver', title: 'Deliver', status: 'pending' }
      ]
    }
    expect(taskBoardWorkItemsProgress(doc)).toBe('4/10')
  })

  it('loop board shows full global_milestones ladder', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_loop',
      meta: {
        goal: 'Batch',
        status: 'running',
        work_items_done: 2,
        work_items_failed: 0,
        work_items_total: 10
      },
      global_milestones: [
        { id: 'g_plan', title: 'Plan', status: 'done' },
        { id: 'wi_1', title: '#1', status: 'done' },
        { id: 'wi_2', title: '#2', status: 'done' },
        { id: 'wi_3', title: '#3', status: 'in_progress' },
        { id: 'g_deliver', title: 'Deliver', status: 'pending' }
      ]
    }
    expect(taskBoardIsLoopMilestoneBoard(doc)).toBe(true)
    expect(taskBoardMilestoneViewMode(doc)).toBe('queue_exec')
    expect(taskBoardVisibleMilestones(doc).map(r => r.id)).toEqual([
      'g_plan',
      'wi_1',
      'wi_2',
      'wi_3',
      'g_deliver'
    ])
    expect(taskBoardVisibleMilestoneProgress(doc)).toBe('3/5')
  })

  it('deliver phase still shows full ladder in UI', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_deliver',
      meta: { goal: 'Batch', status: 'running' },
      global_milestones: [
        { id: 'g_plan', title: 'Plan', status: 'done' },
        { id: 'wi_1', title: '#1', status: 'done' },
        { id: 'g_deliver', title: 'Deliver', status: 'in_progress' }
      ]
    }
    expect(taskBoardMilestoneViewMode(doc)).toBe('queue_deliver')
    expect(taskBoardVisibleMilestones(doc).map(r => r.id)).toEqual([
      'g_plan',
      'wi_1',
      'g_deliver'
    ])
    expect(taskBoardVisibleMilestoneProgress(doc)).toBe('2/3')
  })
})
