import { describe, expect, it } from 'vitest'
import type { TaskBoardDocument } from '../types/chat'
import { taskBoardCompactSummary, taskBoardCollapsedLine } from './taskBoardCollapsedLine'

function doc(goal: string, board: TaskBoardDocument['board'] = []): TaskBoardDocument {
  return {
    version: 2,
    task_id: 'tb_x',
    meta: { goal, status: 'running' },
    board
  }
}

describe('taskBoardCollapsedLine', () => {
  it('taskBoardCompactSummary uses in-progress step as task line', () => {
    const summary = taskBoardCompactSummary(
      doc('打开微信，随机生成10个手机号搜索账号是否存在', [
        { id: '1', title: 'step one', status: 'done', done_when: '打开微信' },
        { id: '2', title: 'step two', status: 'in_progress', done_when: '搜索手机号' },
        { id: '3', title: 'step three', status: 'pending', done_when: '汇总结果' }
      ])
    )
    expect(summary?.progress).toBe('1/3')
    expect(summary?.goal).toContain('打开微信')
    expect(summary?.currentStep).toBe('step two')
    expect(summary?.taskLine).toBe('step two')
    expect(summary?.fullLine).toBe('step two')
    expect(taskBoardCollapsedLine(doc('g', [{ id: '1', title: 'step', status: 'done', done_when: '完成' }]))).toBe('g')
  })

  it('loop exec shows in-progress wi row', () => {
    const summary = taskBoardCompactSummary({
      version: 4,
      task_id: 'tb_loop',
      meta: { goal: 'BOSS 批量', status: 'running' },
      global_milestones: [
        { id: 'g_plan', title: '计划', status: 'done' },
        { id: 'wi_1', title: '深圳', status: 'done' },
        { id: 'wi_2', title: '北京', status: 'in_progress' },
        { id: 'g_deliver', title: '交付', status: 'pending' }
      ]
    })
    expect(summary?.taskLine).toBe('北京')
    expect(summary?.progress).toBe('1/2')
    expect(summary?.fullLine).toBe('北京')
  })

  it('uses loop meta progress when available', () => {
    const summary = taskBoardCompactSummary({
      version: 4,
      task_id: 'tb_wi',
      meta: {
        goal: 'BOSS 批量',
        status: 'running',
        work_items_done: 4,
        work_items_failed: 0,
        work_items_total: 10
      },
      global_milestones: [
        { id: 'g_plan', title: '计划', status: 'done' },
        { id: 'wi_1', title: 'A', status: 'done' },
        { id: 'wi_2', title: 'B', status: 'in_progress' },
        { id: 'g_deliver', title: '交付', status: 'pending' }
      ]
    })
    expect(summary?.progress).toBe('4/10')
    expect(summary?.doneCount).toBe(4)
    expect(summary?.total).toBe(10)
    expect(summary?.taskLine).toBe('B')
  })

  it('deliver phase shows g_deliver in compact summary', () => {
    const summary = taskBoardCompactSummary({
      version: 4,
      task_id: 'tb_deliver',
      meta: { goal: 'BOSS 批量', status: 'running' },
      global_milestones: [
        { id: 'g_plan', title: '计划', status: 'done' },
        { id: 'wi_1', title: 'A', status: 'done' },
        { id: 'g_deliver', title: '交付', status: 'in_progress' }
      ]
    })
    expect(summary?.taskLine).toBe('交付')
    expect(summary?.progress).toBe('0/1')
  })
})
