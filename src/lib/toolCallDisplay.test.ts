import { describe, expect, it } from 'vitest'
import type { ToolCall } from '../types/chat'
import { buildFileChangeSummaries, backgroundJobIdFromToolCall, collapsedLiveRunItemKey, collapsedToolListItems, compactToolCallLiveText, compactToolCallStatusLine, effectiveToolDisplayLabel, effectiveToolDisplaySummary, fileToolDisplayPath, formatCollapsedToolGroupLine, formatToolDurationLabel, isBackgroundJobHandleResult, isBackgroundSubagentCall, isJobAwaitCall, latestToolCallForCompactStatus, partitionCollapsedToolCalls, resolveBackgroundHostDisplayStatus, resolveCollapsedGroupLiveTool, resolveToolDisplayForCall, shouldPinSubAgentHostRow, truncatePathKeepEnd, workspaceRelativeDisplayPath, workspaceRelativeDisplayPathWithFallbacks } from './toolCallDisplay'

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

  it('live text drops the kind name when the icon replaces it', () => {
    const terminal = tc({
      id: '1',
      name: 'terminal',
      status: 'running',
      displayLabel: '终端命令',
      displaySummary: '计算 2 的 0 到 15 次方'
    })
    expect(compactToolCallLiveText(terminal)).toBe('计算 2 的 0 到 15 次方')
    expect(compactToolCallLiveText(
      tc({
        id: 'mu',
        name: 'media_understand',
        status: 'running',
        displayLabel: '媒体理解',
        arguments: JSON.stringify({
          refs: ['pointer-media://c/a.pdf'],
          goal: '识别发票金额'
        })
      })
    )).toBe('识别发票金额')
    expect(compactToolCallLiveText(
      tc({
        id: 'term-label',
        name: 'terminal',
        status: 'running',
        displayLabel: '终端命令',
        arguments: JSON.stringify({
          command: 'ls -la',
          label: '列出当前目录'
        })
      })
    )).toBe('列出当前目录')
    expect(compactToolCallLiveText(
      tc({
        id: '2',
        name: 'wait',
        status: 'running',
        displayLabel: '等待 12 秒'
      })
    )).toBe('等待 12 秒')
    expect(compactToolCallLiveText(
      tc({
        id: '3',
        name: 'run_subagent',
        status: 'running',
        displayLabel: '委派子任务',
        displaySummary: '对比方案 B'
      })
    )).toBe('对比方案 B')
    expect(compactToolCallLiveText(
      tc({
        id: '4',
        name: 'ask_user',
        status: 'running',
        displayLabel: '询问用户'
      })
    )).toBe('询问用户')
    expect(compactToolCallLiveText(
      tc({
        id: '5',
        name: 'mystery_plugin',
        status: 'running',
        displayLabel: 'mystery_plugin'
      })
    )).toBe('mystery_plugin')
  })

  it('shows 后台执行中 for background run_subagent', () => {
    const line = compactToolCallStatusLine(
      tc({
        id: '1',
        name: 'run_subagent',
        status: 'running',
        displayLabel: '委派子任务',
        displaySummary: '探索代码库',
        arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true })
      })
    )
    expect(line).toBe('委派子任务 · 探索代码库 · 后台执行中')
  })

  it('treats self/explore/coder omit background as background host', () => {
    const explore = tc({
      id: '1',
      name: 'run_subagent',
      status: 'success',
      displayLabel: '委派子任务',
      displaySummary: '任务A',
      arguments: JSON.stringify({ agentId: 'explore', goal: 'map', title: '任务A' }),
      result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
    })
    expect(isBackgroundSubagentCall(explore)).toBe(true)
    expect(resolveBackgroundHostDisplayStatus(explore)).toBe('running')
    expect(compactToolCallStatusLine(explore)).toBe('委派子任务 · 任务A · 后台执行中')

    const coder = tc({
      id: '2',
      name: 'run_subagent',
      status: 'running',
      displayLabel: '委派子任务',
      displaySummary: '实现',
      arguments: JSON.stringify({ agentId: 'coder', goal: 'What: patch\nDone when: green' })
    })
    expect(isBackgroundSubagentCall(coder)).toBe(true)
  })

  it('shows 后台执行中 for background terminal', () => {
    const line = compactToolCallStatusLine(
      tc({
        id: '1',
        name: 'terminal',
        status: 'running',
        displayLabel: '终端命令',
        displaySummary: '跑测试',
        arguments: JSON.stringify({ command: 'cargo test', label: '跑测试', blockUntilMs: 0 })
      })
    )
    expect(line).toBe('终端命令 · 跑测试 · 后台执行中')
  })

  it('does not treat job handles as terminal stdout', () => {
    expect(isBackgroundJobHandleResult('{"jobId":"job_1","status":"running","kind":"terminal"}')).toBe(true)
    expect(isBackgroundJobHandleResult('{"jobId":"job_1","status":"running","kind":"subagent"}')).toBe(true)
    expect(isBackgroundJobHandleResult('{"exitCode":0,"success":true,"stdout":"ok"}')).toBe(false)
    expect(isBackgroundJobHandleResult('{"content":"worker markdown"}')).toBe(false)
  })

  it('does not treat job.status snapshots as spawn handles', () => {
    expect(
      isBackgroundJobHandleResult(
        '{"jobId":"job_1","status":"running","kind":"subagent","claimed":false,"title":"coder"}'
      )
    ).toBe(false)
    const statusCall = tc({
      id: 'job-status',
      name: 'job',
      status: 'success',
      displayLabel: '后台任务状态',
      arguments: JSON.stringify({ action: 'status', jobId: 'job_1' }),
      result: '{"jobId":"job_1","status":"running","kind":"subagent","claimed":false}'
    })
    expect(resolveBackgroundHostDisplayStatus(statusCall)).toBe('success')
    expect(compactToolCallStatusLine(statusCall)).toBe('后台任务状态')
  })

  it('parses background job id and job.await', () => {
    expect(
      backgroundJobIdFromToolCall(
        tc({
          id: '1',
          name: 'run_subagent',
          status: 'running',
          result: '{"jobId":"job_abc","status":"running","kind":"subagent"}',
        })
      )
    ).toBe('job_abc')
    expect(
      isJobAwaitCall(
        tc({
          id: '2',
          name: 'job',
          status: 'running',
          arguments: JSON.stringify({ action: 'await', mode: 'any' }),
        })
      )
    ).toBe(true)
    expect(
      isJobAwaitCall(
        tc({
          id: '3',
          name: 'job',
          status: 'running',
          arguments: JSON.stringify({ action: 'list' }),
        })
      )
    ).toBe(false)
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

  it('falls back display for session_search and session_read', () => {
    expect(
      resolveToolDisplayForCall(
        tc({
          id: 'ss',
          name: 'session_search',
          status: 'success',
          arguments: JSON.stringify({ query: '上次改过登录' })
        })
      )
    ).toEqual({ label: '搜索会话', summary: '上次改过登录' })
    expect(
      resolveToolDisplayForCall(
        tc({
          id: 'sr',
          name: 'session_read',
          status: 'success',
          arguments: JSON.stringify({ offset: 12 })
        })
      )
    ).toEqual({ label: '读取会话', summary: '第 12 条' })
    expect(
      effectiveToolDisplayLabel(
        tc({
          id: 'ss-slug',
          name: 'session_search',
          status: 'success',
          displayLabel: 'session_search',
          arguments: JSON.stringify({ query: 'foo' })
        })
      )
    ).toBe('搜索会话')
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
      workspaceRelativeDisplayPath('/Users/alice/.pointer/skills/review/SKILL.md')
    ).toBe('review/SKILL.md')
    expect(
      workspaceRelativeDisplayPath('~/.pointer/skills/review/scripts/check.py')
    ).toBe('review/scripts/check.py')
    expect(
      workspaceRelativeDisplayPath('C:\\Users\\alice\\.pointer\\skills\\review\\SKILL.md')
    ).toBe('review/SKILL.md')
    expect(
      workspaceRelativeDisplayPath('/Users/alice/.pointer/cache/index.json')
    ).toBe('/Users/alice/.pointer/cache/index.json')
  })

  it('prefers the active workspace when it is inside the Pointer directory', () => {
    expect(
      workspaceRelativeDisplayPath(
        '/Users/alice/.pointer/skills/review/scripts/check.py',
        '/Users/alice/.pointer/skills/review'
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

  it('falls back to project / sandbox roots when the lead workspace is a skill dir', () => {
    const sandbox =
      '/Users/alice/Library/Application Support/PointerApp/session-sandboxes/1530c681-176d-40ca-84b4-a90a34312628'
    const skill = '/Users/alice/.pointer/skills/cwpt-reimburse-submit'
    const file = `${sandbox}/_year_repro/repro.py`
    expect(workspaceRelativeDisplayPath(file, skill)).toBe(file.replace(/\\/g, '/'))
    expect(
      workspaceRelativeDisplayPathWithFallbacks(file, [skill, sandbox])
    ).toBe('_year_repro/repro.py')
    expect(
      workspaceRelativeDisplayPathWithFallbacks(file, [skill])
    ).toBe('_year_repro/repro.py')
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
    expect(compactToolCallLiveText(
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
    )).not.toContain('编辑文件')
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

  it('pins running background hosts above collapsed groups', () => {
    const items = partitionCollapsedToolCalls([
      tc({ id: '1', name: 'file_read', status: 'success' }),
      tc({ id: '2', name: 'file_read', status: 'success' }),
      tc({
        id: '3',
        name: 'run_subagent',
        status: 'running',
        arguments: JSON.stringify({ background: true, prompt: 'scan repo' })
      }),
      tc({ id: '4', name: 'file_grep', status: 'success' })
    ])
    expect(items.map(i => i.kind)).toEqual(['single', 'group', 'single'])
    if (items[0]?.kind === 'single') {
      expect(items[0].tool.id).toBe('3')
    }
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

  it('keeps a trailing thinking group after completed ask_user', () => {
    const items = collapsedToolListItems(
      [
        tc({ id: '1', name: 'file_read', status: 'success' }),
        tc({ id: '2', name: 'file_read', status: 'success' }),
        tc({ id: '3', name: 'ask_user', status: 'success' })
      ],
      { holdLiveSlot: true, thinkingLine: '思考中.' }
    )
    expect(items.map(i => i.kind)).toEqual(['group', 'single', 'group'])
    expect(items[1]?.kind === 'single' && items[1].tool.name).toBe('ask_user')
    expect(items[2]).toEqual({ kind: 'group', tools: [] })
  })

  it('does not invent a trailing thinking group for a lone in-flight run_subagent', () => {
    const items = collapsedToolListItems(
      [
        tc({
          id: '1',
          name: 'run_subagent',
          status: 'running',
          arguments: JSON.stringify({ background: true, agentId: 'explore' })
        })
      ],
      { holdLiveSlot: true, thinkingLine: '思考中.........' }
    )
    expect(items.map(i => i.kind)).toEqual(['single'])
    expect(items[0]?.kind === 'single' && items[0].tool.name).toBe('run_subagent')
  })

  it('does not invent trailing thinking when a prior group is pinned behind a live host', () => {
    const items = collapsedToolListItems(
      [
        tc({ id: '1', name: 'file_grep', status: 'success' }),
        tc({
          id: '2',
          name: 'run_subagent',
          status: 'running',
          arguments: JSON.stringify({ background: true, agentId: 'explore' })
        })
      ],
      { holdLiveSlot: true, thinkingLine: '思考中.........' }
    )
    // In-progress background hosts are pinned first; last item is the finished group.
    expect(items.map(i => i.kind)).toEqual(['single', 'group'])
    expect(items.some(i => i.kind === 'group' && i.tools.length === 0)).toBe(false)
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

  it('keeps activity counts while a tool is still running', () => {
    const line = formatCollapsedToolGroupLine([
      tc({ id: '1', name: 'web_search', status: 'failed', displaySummary: '旧查询' }),
      tc({ id: '2', name: 'web_search', status: 'success', displaySummary: '已完成' }),
      tc({
        id: '3',
        name: 'web_search',
        status: 'running',
        displayLabel: '联网搜索',
        displaySummary: '招标投标法实施条例 最新修订'
      }),
      tc({ id: '4', name: 'web_search', status: 'success', displaySummary: '另一次' })
    ])
    expect(line).toBe('探索 4 次搜索')
    expect(line).not.toContain('招标投标法')
    expect(line).not.toContain('执行中')
  })
})

describe('resolveCollapsedGroupLiveTool', () => {
  it('prefers the partitioned live tool', () => {
    const live = tc({ id: 'live', name: 'web_search', status: 'running' })
    const done = tc({ id: 'done', name: 'web_search', status: 'success' })
    expect(resolveCollapsedGroupLiveTool([done], live)?.id).toBe('live')
  })

  it('finds mid-group in-progress when parallel tools finish out of order', () => {
    const tools = [
      tc({ id: '1', name: 'web_search', status: 'failed' }),
      tc({ id: '2', name: 'web_search', status: 'running', displaySummary: '进行中查询' }),
      tc({ id: '3', name: 'web_search', status: 'success' })
    ]
    expect(resolveCollapsedGroupLiveTool(tools, null)?.id).toBe('2')
  })
})
