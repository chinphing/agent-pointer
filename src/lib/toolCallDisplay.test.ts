import { describe, expect, it } from 'vitest'
import type { ToolCall } from '../types/chat'
import { buildFileChangeSummaries, compactToolCallStatusLine, effectiveToolDisplayLabel, effectiveToolDisplaySummary, fileToolDisplayPath, latestToolCallForCompactStatus, resolveToolDisplayForCall, workspaceRelativeDisplayPath } from './toolCallDisplay'

function tc(partial: Partial<ToolCall> & Pick<ToolCall, 'id' | 'name' | 'status'>): ToolCall {
  return {
    arguments: '',
    ...partial
  }
}

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
