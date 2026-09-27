import { describe, expect, it } from 'vitest'
import { isComputerToolName } from './computerExecuting'

describe('isComputerToolName', () => {
  it('matches flat mouse / input / hotkey tools', () => {
    expect(isComputerToolName('mouse_click_index')).toBe(true)
    expect(isComputerToolName('input_focused')).toBe(true)
    expect(isComputerToolName('hotkey')).toBe(true)
    expect(isComputerToolName('wait')).toBe(true)
  })

  it('matches app access and clipboard tools used by computer sub-agent', () => {
    expect(isComputerToolName('launch_app')).toBe(true)
    expect(isComputerToolName('list_apps')).toBe(true)
    expect(isComputerToolName('clipboard_read')).toBe(true)
    expect(isComputerToolName('clipboard_write')).toBe(true)
  })

  it('rejects unrelated tools', () => {
    expect(isComputerToolName('file_read')).toBe(false)
    expect(isComputerToolName('run_subagent')).toBe(false)
  })
})
