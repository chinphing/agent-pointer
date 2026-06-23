import { describe, expect, it } from 'vitest'
import type { TaskBoardItem } from '../types/chat'
import { milestoneTitle } from './taskBoardDisplay'

describe('taskBoardDisplay', () => {
  it('shows title only', () => {
    const item: TaskBoardItem = {
      id: 'g_plan',
      title: '读取 Excel 并打开 BOSS 职位发布页',
      status: 'done',
      done_when: 'criteria not shown in UI'
    }
    expect(milestoneTitle(item)).toBe('读取 Excel 并打开 BOSS 职位发布页')
  })

  it('falls back to id when title empty', () => {
    const item: TaskBoardItem = { id: 'm1', title: '', status: 'pending' }
    expect(milestoneTitle(item)).toBe('m1')
  })
})
