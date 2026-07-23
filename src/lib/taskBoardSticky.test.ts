import { describe, expect, it } from 'vitest'
import { shouldStickActiveTaskBoard } from './taskBoardSticky'

describe('shouldStickActiveTaskBoard', () => {
  it('keeps a running parent board inline before it reaches the top', () => {
    expect(shouldStickActiveTaskBoard(99, 100, true)).toBe(false)
  })

  it('sticks a running parent board once its inline position crosses the top', () => {
    expect(shouldStickActiveTaskBoard(100, 100, true)).toBe(true)
    expect(shouldStickActiveTaskBoard(120, 100, true)).toBe(true)
  })

  it('waits until the inline position is measured', () => {
    expect(shouldStickActiveTaskBoard(120, null, true)).toBe(false)
  })

  it('never sticks a terminal or inactive board', () => {
    expect(shouldStickActiveTaskBoard(120, 100, false)).toBe(false)
  })
})
