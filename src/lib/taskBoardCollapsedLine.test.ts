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

  it('falls back to goal when no step is in progress', () => {
    const summary = taskBoardCompactSummary({
      version: 4,
      task_id: 'tb_v4',
      meta: { goal: 'BOSS 批量', status: 'running', work_item_mode: 'enumerated' },
      global_milestones: [
        { id: 'g_plan', title: '计划', status: 'done', done_when: '读取 Excel 并打开 BOSS' },
        { id: 'g_exec', title: '执行', status: 'in_progress', done_when: '所有城市地址添加完成' },
        { id: 'g_deliver', title: '交付', status: 'ready', done_when: '汇报最终结果' }
      ],
      item_milestones: [
        { id: 'm1', title: '添加工作地址', status: 'in_progress' },
        { id: 'm2', title: '确认保存', status: 'pending' }
      ]
    })
    expect(summary?.taskLine).toBe('添加工作地址')
    expect(summary?.progress).toBe('0/2')
    expect(summary?.fullLine).toBe('添加工作地址')
    expect(summary?.doneCount).toBe(0)
  })

  it('deliver phase shows g_deliver in compact summary', () => {
    const summary = taskBoardCompactSummary({
      version: 4,
      task_id: 'tb_deliver',
      meta: { goal: 'BOSS 批量', status: 'running', work_item_mode: 'enumerated' },
      global_milestones: [
        { id: 'g_plan', title: '计划', status: 'done' },
        { id: 'g_exec', title: '执行', status: 'done' },
        { id: 'g_deliver', title: '交付', status: 'in_progress' }
      ],
      item_milestones: [{ id: 'm1', title: 'SOP', status: 'pending' }]
    })
    expect(summary?.taskLine).toBe('交付')
    expect(summary?.progress).toBe('0/1')
  })
})
