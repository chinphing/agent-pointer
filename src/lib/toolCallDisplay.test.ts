import { describe, expect, it } from 'vitest'
import type { ToolCall } from '../types/chat'
import { buildFileChangeSummaries, collapsedLiveRunItemKey, collapsedToolListItems, compactToolCallStatusLine, effectiveToolDisplayLabel, effectiveToolDisplaySummary, fileToolDisplayPath, formatCollapsedToolGroupLine, formatToolDurationLabel, latestToolCallForCompactStatus, partitionCollapsedToolCalls, resolveToolDisplayForCall, shouldPinSubAgentHostRow, truncatePathKeepEnd, workspaceRelativeDisplayPath } from './toolCallDisplay'

function tc(partial: Partial<ToolCall> & Pick<ToolCall, 'id' | 'name' | 'status'>): ToolCall {
  return {
    arguments: '',
    ...partial
  }
}

describe('formatToolDurationLabel', () => {
  it('hides under one second', () => {
    expect(formatToolDurationLabel(undefined)).toBe('')
    expect(formatToolDurationLabel(0)).toBe('')
    expect(formatToolDurationLabel(999)).toBe('')
  })

  it('shows whole seconds only', () => {
    expect(formatToolDurationLabel(1000)).toBe('1s')
    expect(formatToolDurationLabel(1926)).toBe('1s')
    expect(formatToolDurationLabel(2000)).toBe('2s')
  })
})

describe('compactToolCallStatusLine', () => {
  it('formats running tool like expanded ToolCallRow', () => {
    const line = compactToolCallStatusLine(
      tc({
        id: '1',
        name: 'mouse',
        status: 'running',
        displayLabel: '鼠标',
        displaySummary: '点击微信图标'
      })
    )
    expect(line).toBe('鼠标 · 点击微信图标 · 执行中')
  })

  it('omits running status on the collapsed current-task line', () => {
    const line = compactToolCallStatusLine(
      tc({
        id: '1',
        name: 'terminal',
        status: 'running',
        displayLabel: '终端命令',
        displaySummary: '计算 2 的 0 到 15 次方'
      }),
      undefined,
      { includeStatus: false }
    )
    expect(line).toBe('终端命令 · 计算 2 的 0 到 15 次方')
  })

  it('formats success tool without duration suffix', () => {
    const line = compactToolCallStatusLine(
      tc({
        id: '2',
        name: 'mouse',
        status: 'success',
        displayLabel: '鼠标',
        displaySummary: '点击微信图标',
        durationMs: 1926
      })
    )
    expect(line).toBe('鼠标 · 点击微信图标')
  })

  it('latestToolCallForCompactStatus prefers in-progress over completed', () => {
    const calls = [
      tc({ id: '1', name: 'mouse', status: 'success', displayLabel: '鼠标' }),
      tc({ id: '2', name: 'wait', status: 'running', displayLabel: '等待' })
    ]
    expect(latestToolCallForCompactStatus(calls)?.displayLabel).toBe('等待')
    expect(latestToolCallForCompactStatus([calls[0]])?.displayLabel).toBe('鼠标')
  })

  it('falls back display for launch_app without backend labels', () => {
    const d = resolveToolDisplayForCall(
      tc({
        id: '3',
        name: 'launch_app',
        status: 'success',
        arguments: JSON.stringify({ goal: '打开微信', app: 'WeChat' })
      })
    )
    expect(d.label).toBe('启动应用')
    expect(d.summary).toBe('WeChat')
  })

  it('falls back display for cron_job without backend labels', () => {
    const d = resolveToolDisplayForCall(
      tc({
        id: '4',
        name: 'cron_job',
        status: 'success',
        arguments: JSON.stringify({
          action: 'create',
          prompt_text: '每天检查邮件',
          schedule: 'daily@9:30'
        })
      })
    )
    expect(d.label).toBe('创建定时任务')
    expect(d.summary).toBe('daily@9:30')
  })

  it('ask_user keeps question out of tool header summary', () => {
    const call = tc({
      id: '5',
      name: 'ask_user',
      status: 'success',
      arguments: JSON.stringify({
        question: '是否允许桌面控制？',
        options: [{ label: '允许' }, { label: '仅步骤' }]
      }),
      displaySummary: '是否允许桌面控制？'
    })
    expect(resolveToolDisplayForCall(call)).toEqual({ label: '询问用户', summary: '' })
    expect(effectiveToolDisplaySummary(call)).toBe('')
  })

  it('effectiveToolDisplayLabel ignores slug displayLabel from backend', () => {
    expect(
      effectiveToolDisplayLabel(
        tc({
          id: '5',
          name: 'cron_job',
          status: 'success',
          displayLabel: 'cron_job',
          displaySummary: 'cron-cb020203fab3',
          arguments: JSON.stringify({ action: 'disable', job_id: 'cron-cb020203fab3' })
        })
      )
    ).toBe('停用定时任务')
    expect(
      effectiveToolDisplaySummary(
        tc({
          id: '5',
          name: 'cron_job',
          status: 'success',
          displayLabel: 'cron_job',
          displaySummary: 'cron-cb020203fab3',
          arguments: JSON.stringify({ action: 'disable', job_id: 'cron-cb020203fab3' })
        })
      )
    ).toBe('')
  })
})

describe('workspace-relative file tool paths', () => {
  it('normalizes Windows paths and strips a case-insensitive workspace prefix', () => {
    expect(
      workspaceRelativeDisplayPath(
        '\\\\?\\C:\\Project\\Pointer-App\\src\\components\\App.vue',
        'c:\\project\\pointer-app'
      )
    ).toBe('src/components/App.vue')
  })

  it('shortens paths under the user Pointer directory across path styles', () => {
    expect(
      workspaceRelativeDisplayPath('/Users/starliu/.pointer/skills/review/SKILL.md')
    ).toBe('review/SKILL.md')
    expect(
      workspaceRelativeDisplayPath('~/.pointer/skills/review/scripts/check.py')
    ).toBe('review/scripts/check.py')
    expect(
      workspaceRelativeDisplayPath('C:\\Users\\starliu\\.pointer\\skills\\review\\SKILL.md')
    ).toBe('review/SKILL.md')
    expect(
      workspaceRelativeDisplayPath('/Users/starliu/.pointer/cache/index.json')
    ).toBe('/Users/starliu/.pointer/cache/index.json')
  })

  it('prefers the active workspace when it is inside the Pointer directory', () => {
    expect(
      workspaceRelativeDisplayPath(
        '/Users/starliu/.pointer/skills/review/scripts/check.py',
        '/Users/starliu/.pointer/skills/review'
      )
    ).toBe('scripts/check.py')
  })

  it('keeps relative and outside-workspace paths without inventing containment', () => {
    expect(workspaceRelativeDisplayPath('src\\App.vue', 'C:\\project\\pointer-app')).toBe('src/App.vue')
    expect(
      workspaceRelativeDisplayPath('C:\\other\\App.vue', 'C:\\project\\pointer-app')
    ).toBe('C:/other/App.vue')
    expect(
      workspaceRelativeDisplayPath('/tmp/project/.pointer/skills/local/SKILL.md')
    ).toBe('/tmp/project/.pointer/skills/local/SKILL.md')
  })

  it('extracts the path from file tool arguments only', () => {
    expect(
      fileToolDisplayPath(
        tc({
          id: 'path-1',
          name: 'file_edit',
          status: 'success',
          arguments: JSON.stringify({ path: 'C:\\project\\pointer-app\\src\\App.vue' })
        }),
        'C:\\project\\pointer-app'
      )
    ).toBe('src/App.vue')
    expect(
      fileToolDisplayPath(
        tc({
          id: 'path-2',
          name: 'terminal',
          status: 'success',
          arguments: JSON.stringify({ path: 'C:\\project\\pointer-app\\src\\App.vue' })
        }),
        'C:\\project\\pointer-app'
      )
    ).toBe('')
  })

  it('keeps the filename when a tool-row path is too long', () => {
    const path = 'scripts/cwpt/flows/travel_reimburse/standard_query.py'
    expect(truncatePathKeepEnd(path, 40)).toBe('…lows/travel_reimburse/standard_query.py')
    expect(truncatePathKeepEnd(path, 40).endsWith('standard_query.py')).toBe(true)
    const line = compactToolCallStatusLine(
      tc({
        id: 'path-3',
        name: 'file_edit',
        status: 'success',
        displayLabel: '编辑文件',
        arguments: JSON.stringify({
          path: '/tmp/project/scripts/cwpt/flows/travel_reimburse/standard_query.py'
        })
      }),
      '/tmp/project'
    )
    expect(line.startsWith('编辑文件 · ')).toBe(true)
    expect(line.endsWith('standard_query.py')).toBe(true)
    expect(line).not.toContain('scripts/cwpt/flows/travel_reimburse/standard_query.py')
  })
})

describe('buildFileChangeSummaries', () => {
  function editResult(path: string, adds: number, dels: number): string {
    return JSON.stringify({
      path,
      success: true,
      replaced: 1,
      stats: { adds, dels }
    })
  }

  function writeResult(path: string): string {
    return JSON.stringify({
      path,
      success: true,
      bytesWritten: 12,
      created: true
    })
  }

  it('aggregates successful changes by normalized path', () => {
    const summaries = buildFileChangeSummaries([
      tc({
        id: 'edit-1',
        name: 'file_edit',
        status: 'success',
        arguments: JSON.stringify({ path: 'src\\App.vue' }),
        result: editResult('src\\App.vue', 2, 1)
      }),
      tc({
        id: 'edit-2',
        name: 'file_edit',
        status: 'success',
        arguments: JSON.stringify({ path: 'src/App.vue' }),
        result: editResult('src/App.vue', 3, 4)
      }),
      tc({
        id: 'write-1',
        name: 'file_write',
        status: 'success',
        arguments: JSON.stringify({ path: 'src/New.vue', content: 'a\nb\n' }),
        result: writeResult('src/New.vue')
      })
    ])

    expect(summaries).toHaveLength(2)
    expect(summaries[0]).toMatchObject({
      path: 'src\\App.vue',
      fileName: 'App.vue',
      adds: 5,
      dels: 5
    })
    expect(summaries[0].diffs).toHaveLength(0)
    expect(summaries[1]).toMatchObject({
      path: 'src/New.vue',
      fileName: 'New.vue',
      kind: 'write',
      adds: 2,
      dels: 0
    })
  })

  it('ignores failed, running, and malformed file changes', () => {
    const summaries = buildFileChangeSummaries([
      tc({
        id: 'failed',
        name: 'file_edit',
        status: 'failed',
        result: editResult('failed.ts', 1, 1)
      }),
      tc({
        id: 'running',
        name: 'file_write',
        status: 'running',
        result: writeResult('running.ts')
      }),
      tc({
        id: 'malformed',
        name: 'file_edit',
        status: 'success',
        result: '{bad json'
      }),
      tc({
        id: 'terminal',
        name: 'terminal',
        status: 'success',
        result: editResult('not-a-file-tool.ts', 1, 1)
      })
    ])

    expect(summaries).toEqual([])
  })
})

describe('partitionCollapsedToolCalls', () => {
  it('collapses consecutive finished file tools', () => {
    const items = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'file_grep', status: 'success' }),
      tc({ id: '3', name: 'file_read', status: 'success' })
    ])
    expect(items).toHaveLength(1)
    expect(items[0]?.kind).toBe('group')
    if (items[0]?.kind === 'group') {
      expect(items[0].tools.map(t => t.id)).toEqual(['1', '2', '3'])
    }
  })

  it('collapses a single finished tool when a live tool follows', () => {
    const items = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'terminal', status: 'running' })
    ])
    expect(items).toHaveLength(1)
    expect(items[0]?.kind).toBe('group')
    if (items[0]?.kind === 'group') {
      expect(items[0].tools.map(t => t.id)).toEqual(['1'])
      expect(items[0].live?.id).toBe('2')
    }
  })

  it('keeps the same group item when the trailing live tool changes', () => {
    const first = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'file_read', status: 'success' }),
      tc({ id: '3', name: 'terminal', status: 'running' })
    ])
    const next = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'file_read', status: 'success' }),
      tc({ id: '3', name: 'terminal', status: 'success' }),
      tc({ id: '4', name: 'file_grep', status: 'running' })
    ])
    expect(first).toHaveLength(1)
    expect(next).toHaveLength(1)
    expect(first[0]?.kind === 'group' && first[0].live?.id).toBe('3')
    expect(next[0]?.kind === 'group' && next[0].tools.map(t => t.id)).toEqual(['1', '2', '3'])
    expect(next[0]?.kind === 'group' && next[0].live?.id).toBe('4')
  })

  it('does not collapse a single finished tool', () => {
    const items = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' })
    ])
    expect(items).toEqual([{ kind: 'single', tool: expect.objectContaining({ id: '1' }) }])
  })

  it('does not collapse a lone in-progress tool into an empty group header', () => {
    const items = partitionCollapsedToolCalls(
      [tc({ id: '1', name: 'terminal', status: 'running' })],
      { holdLiveSlot: true }
    )
    expect(items).toEqual([{ kind: 'single', tool: expect.objectContaining({ id: '1' }) }])
  })

  it('holds a single finished tool as a group while the run is still live', () => {
    const items = partitionCollapsedToolCalls(
      [tc({ id: '1', name: 'file_read', status: 'success' })],
      { holdLiveSlot: true }
    )
    expect(items).toHaveLength(1)
    expect(items[0]?.kind).toBe('group')
    if (items[0]?.kind === 'group') {
      expect(items[0].tools.map(t => t.id)).toEqual(['1'])
      expect(items[0].live).toBeUndefined()
    }
  })

  it('promotes a finished tool into a group after the first live tool completes', () => {
    const live = partitionCollapsedToolCalls(
      [tc({ id: '1', name: 'terminal', status: 'running' })],
      { holdLiveSlot: true }
    )
    const done = partitionCollapsedToolCalls(
      [tc({ id: '1', name: 'terminal', status: 'success' })],
      { holdLiveSlot: true }
    )
    expect(live[0]?.kind).toBe('single')
    expect(done[0]?.kind).toBe('group')
    if (done[0]?.kind === 'group') {
      expect(done[0].tools.map(t => t.id)).toEqual(['1'])
    }
  })

  it('collapses media_understand with neighboring process tools', () => {
    const items = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'media_understand', status: 'success' }),
      tc({ id: '3', name: 'file_grep', status: 'success' })
    ])
    expect(items).toHaveLength(1)
    expect(items[0]?.kind).toBe('group')
  })

  it('still splits around image_generate', () => {
    const items = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'file_read', status: 'success' }),
      tc({ id: '3', name: 'image_generate', status: 'success' }),
      tc({ id: '4', name: 'file_read', status: 'success' }),
      tc({ id: '5', name: 'file_read', status: 'success' })
    ])
    expect(items.map(i => i.kind)).toEqual(['group', 'single', 'group'])
  })

  it('splits around ask_user and run_subagent', () => {
    const items = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'file_read', status: 'success' }),
      tc({ id: '3', name: 'ask_user', status: 'success' }),
      tc({ id: '4', name: 'run_subagent', status: 'success' }),
      tc({ id: '5', name: 'file_read', status: 'success' }),
      tc({ id: '6', name: 'file_grep', status: 'success' })
    ])
    expect(items.map(i => i.kind)).toEqual(['group', 'single', 'single', 'group'])
  })

  it('pins the host run_subagent row only while the user must act', () => {
    expect(shouldPinSubAgentHostRow(tc({ id: '1', name: 'run_subagent', status: 'success' }))).toBe(false)
    expect(shouldPinSubAgentHostRow(tc({ id: '2', name: 'run_subagent', status: 'pending_approval' }))).toBe(true)
    expect(
      shouldPinSubAgentHostRow(tc({ id: '3', name: 'run_subagent', status: 'running', waitingForInput: true }))
    ).toBe(true)
  })

  it('collapses consecutive finished tools from later rounds in one list', () => {
    const round1 = [
      tc({ id: '1', name: 'file_grep', status: 'success' }),
      tc({ id: '2', name: 'file_list', status: 'success' })
    ]
    const round2 = [
      tc({ id: '3', name: 'file_read', status: 'success' }),
      tc({ id: '4', name: 'file_read', status: 'success' }),
      tc({ id: '5', name: 'file_read', status: 'success' }),
      tc({ id: '6', name: 'file_read', status: 'success' })
    ]
    const items = partitionCollapsedToolCalls([...round1, ...round2])
    expect(items).toHaveLength(1)
    expect(items[0]?.kind).toBe('group')
    if (items[0]?.kind === 'group') {
      expect(items[0].tools.map(t => t.id)).toEqual(['1', '2', '3', '4', '5', '6'])
    }
  })
})

describe('collapsedToolListItems', () => {
  it('emits an empty live group so first-round thinking occupies the run header', () => {
    const items = collapsedToolListItems([], {
      holdLiveSlot: true,
      thinkingLine: '思考中.'
    })
    expect(items).toEqual([{ kind: 'group', tools: [] }])
  })

  it('does not invent a group when the run is not live', () => {
    expect(collapsedToolListItems([], { thinkingLine: '思考中.' })).toEqual([])
  })

  it('switches from the thinking header to a single-tool row key', () => {
    const thinking = collapsedToolListItems([], {
      holdLiveSlot: true,
      thinkingLine: '思考中.'
    })
    const live = collapsedToolListItems(
      [tc({ id: '1', name: 'terminal', status: 'running' })],
      { holdLiveSlot: true, thinkingLine: '思考中.' }
    )
    expect(collapsedLiveRunItemKey(thinking[0]!, 0, thinking.length, true)).toBe('group:live-run')
    expect(live[0]?.kind).toBe('single')
    expect(collapsedLiveRunItemKey(live[0]!, 0, live.length, true)).toBe('1')
  })
})

describe('formatCollapsedToolGroupLine', () => {
  it('summarizes exploration like Cursor', () => {
    const line = formatCollapsedToolGroupLine([
      tc({
        id: '1',
        name: 'file_read',
        status: 'success',
        arguments: JSON.stringify({ path: 'a.ts' })
      }),
      tc({
        id: '2',
        name: 'file_read',
        status: 'success',
        arguments: JSON.stringify({ path: 'b.ts' })
      }),
      tc({ id: '3', name: 'file_grep', status: 'success' }),
      tc({ id: '4', name: 'terminal', status: 'success' })
    ])
    expect(line).toBe('探索 2 个文件，1 次搜索，执行 1 条命令')
  })

  it('counts media_understand in the collapsed line', () => {
    const line = formatCollapsedToolGroupLine([
      tc({ id: '1', name: 'media_understand', status: 'success' }),
      tc({ id: '2', name: 'media_understand', status: 'success' })
    ])
    expect(line).toBe('理解 2 次')
  })
})
