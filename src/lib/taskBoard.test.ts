import { describe, expect, it } from 'vitest'
import type { TaskBoardDocument } from '../types/chat'
import {
  hasTaskBoardContent,
  milestoneShowsRunning,
  nestedBoardMetaStatus,
  taskBoardCurrentMilestone,
  taskBoardExecutionLabel,
  taskBoardGlobalMilestones,
  taskBoardVisibleMilestoneProgress,
  taskBoardVisibleMilestones
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

  it('linear board progress counts all milestones', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_linear',
      meta: { goal: 'g', status: 'running' },
      global_milestones: [
        { id: 'm1', title: 'One', status: 'done' },
        { id: 'm2', title: 'Two', status: 'in_progress' }
      ]
    }
    expect(taskBoardVisibleMilestones(doc).map(r => r.id)).toEqual(['m1', 'm2'])
    expect(taskBoardVisibleMilestoneProgress(doc)).toBe('1/2')
  })

  it('treats first ready as current when no in_progress on a running board', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_ready_only',
      meta: { goal: 'g', status: 'running' },
      global_milestones: [
        { id: 'm1', title: 'One', status: 'done' },
        { id: 'm2', title: 'Two', status: 'ready' },
        { id: 'm3', title: 'Three', status: 'ready' }
      ]
    }
    const current = taskBoardCurrentMilestone(doc)
    expect(current?.id).toBe('m2')
    expect(milestoneShowsRunning(doc.global_milestones![1]!, current)).toBe(true)
    expect(milestoneShowsRunning(doc.global_milestones![2]!, current)).toBe(false)
  })

  it('loop board progress counts full ladder including bookends', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_loop',
      meta: { goal: 'Batch', status: 'running' },
      global_milestones: [
        { id: 'g_plan', title: 'Plan', status: 'done' },
        { id: 'wi_1', title: '#1', status: 'done' },
        { id: 'wi_2', title: '#2', status: 'done' },
        { id: 'wi_3', title: '#3', status: 'in_progress' },
        { id: 'g_deliver', title: 'Deliver', status: 'pending' }
      ]
    }
    expect(taskBoardVisibleMilestones(doc).map(r => r.id)).toEqual([
      'g_plan',
      'wi_1',
      'wi_2',
      'wi_3',
      'g_deliver'
    ])
    expect(taskBoardVisibleMilestoneProgress(doc)).toBe('3/5')
  })

  it('does not treat in-progress rows as current after the board is abandoned', () => {
    const doc: TaskBoardDocument = {
      version: 4,
      task_id: 'tb_abandoned',
      meta: { goal: 'old batch', status: 'failed' },
      global_milestones: [
        { id: 'm1', title: 'One', status: 'done' },
        { id: 'm2', title: 'Two', status: 'in_progress' },
        { id: 'm3', title: 'Three', status: 'pending' }
      ]
    }
    expect(taskBoardCurrentMilestone(doc)).toBeNull()
    expect(milestoneShowsRunning(doc.global_milestones![1]!, null, 'failed')).toBe(false)
  })

  it('maps board meta to execution labels and never paints active', () => {
    expect(taskBoardExecutionLabel('running')).toBe('执行中')
    expect(taskBoardExecutionLabel('active')).toBe('执行中')
    expect(taskBoardExecutionLabel('failed')).toBe('失败')
    expect(taskBoardExecutionLabel('paused')).toBe('已暂停')
    expect(taskBoardExecutionLabel('completed')).toBeNull()
    expect(taskBoardExecutionLabel('cancelled')).toBe('已取消')
  })

  it('nested board follows the sub-agent trace over leftover meta.running', () => {
    expect(nestedBoardMetaStatus('running', 'completed')).toBe('completed')
    expect(nestedBoardMetaStatus('running', 'failed')).toBe('failed')
    expect(nestedBoardMetaStatus('running', 'cancelled')).toBe('cancelled')
    expect(nestedBoardMetaStatus('running', 'running')).toBe('running')
    expect(nestedBoardMetaStatus('completed', undefined)).toBe('completed')
  })
})
