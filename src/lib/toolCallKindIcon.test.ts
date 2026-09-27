import { describe, expect, it } from 'vitest'
import { History, Hourglass, MessageSquareText, Search, Wrench } from 'lucide-vue-next'
import {
  isKnownToolKind,
  toolCallKindIcon,
  toolCallShowsKindLabel
} from './toolCallKindIcon'

describe('toolCallKindIcon', () => {
  it('maps known kinds and keeps labels only for 询问 / unknown', () => {
    expect(isKnownToolKind('terminal')).toBe(true)
    expect(isKnownToolKind('file_read')).toBe(true)
    expect(isKnownToolKind('mouse_click_at')).toBe(true)
    expect(isKnownToolKind('clipboard_read')).toBe(true)
    expect(isKnownToolKind('skill_read')).toBe(true)
    expect(isKnownToolKind('task_board_patch')).toBe(true)
    expect(isKnownToolKind('read_lints')).toBe(true)
    expect(isKnownToolKind('session_search')).toBe(true)
    expect(isKnownToolKind('session_read')).toBe(true)
    expect(isKnownToolKind('job')).toBe(true)
    expect(isKnownToolKind('mystery_plugin')).toBe(false)
    expect(toolCallKindIcon('mystery_plugin')).toBe(Wrench)
    expect(toolCallKindIcon('session_search')).toBe(History)
    expect(toolCallKindIcon('session_read')).toBe(MessageSquareText)
    expect(toolCallKindIcon('file_grep')).toBe(Search)
    expect(toolCallKindIcon('job')).toBe(Hourglass)

    expect(toolCallShowsKindLabel('terminal')).toBe(false)
    expect(toolCallShowsKindLabel('wait')).toBe(false)
    expect(toolCallShowsKindLabel('run_subagent')).toBe(false)
    expect(toolCallShowsKindLabel('ask_user')).toBe(true)
    expect(toolCallShowsKindLabel('job')).toBe(true)
    expect(toolCallShowsKindLabel('mystery_plugin')).toBe(true)
  })
})
