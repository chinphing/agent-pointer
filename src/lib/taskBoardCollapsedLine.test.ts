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
  it('taskBoardCompactSummary exposes progress separately from long goal', () => {
    const summary = taskBoardCompactSummary(
      doc('打开微信，随机生成10个手机号搜索账号是否存在', [
        { id: '1', title: 'step one', status: 'done' },
        { id: '2', title: 'step two', status: 'in_progress' },
        { id: '3', title: 'step three', status: 'pending' }
      ])
    )
    expect(summary?.progress).toBe('1/3')
    expect(summary?.goal).toContain('打开微信')
    expect(summary?.currentStep).toBe('step two')
    expect(summary?.taskLine).toBe('step two')
    expect(taskBoardCollapsedLine(doc('g', [{ id: '1', status: 'done' }]))).toBe('g · 1/1')
  })
})
