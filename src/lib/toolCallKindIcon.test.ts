import { describe, expect, it } from 'vitest'
import { Wrench } from 'lucide-vue-next'
import {
  isKnownToolKind,
  toolCallKindIcon,
  toolCallShowsKindLabel
} from './toolCallKindIcon'

describe('toolCallKindIcon', () => {
  it('maps known kinds and keeps labels only for 委派 / 询问 / unknown', () => {
    expect(isKnownToolKind('terminal')).toBe(true)
    expect(isKnownToolKind('file_read')).toBe(true)
    expect(isKnownToolKind('mouse_click_at')).toBe(true)
    expect(isKnownToolKind('clipboard_read')).toBe(true)
    expect(isKnownToolKind('captcha_verify_click')).toBe(true)
    expect(isKnownToolKind('skill_read')).toBe(true)
    expect(isKnownToolKind('task_board_patch')).toBe(true)
    expect(isKnownToolKind('read_lints')).toBe(true)
    expect(isKnownToolKind('mystery_plugin')).toBe(false)
    expect(toolCallKindIcon('mystery_plugin')).toBe(Wrench)

    expect(toolCallShowsKindLabel('terminal')).toBe(false)
    expect(toolCallShowsKindLabel('wait')).toBe(false)
    expect(toolCallShowsKindLabel('run_subagent')).toBe(true)
    expect(toolCallShowsKindLabel('ask_user')).toBe(true)
    expect(toolCallShowsKindLabel('mystery_plugin')).toBe(true)
  })
})
