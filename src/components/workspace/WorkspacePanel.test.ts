// @vitest-environment happy-dom

import { createApp, nextTick } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'

const apiMocks = vi.hoisted(() => ({
  getWorkspaceGitStatus: vi.fn(async () => ({ changes: [] })),
  listWorkspaceDirectory: vi.fn(async () => [
    { name: 'README.md', path: 'README.md', kind: 'file', sizeBytes: 64 }
  ]),
  readWorkspaceFile: vi.fn()
}))
const chatState = vi.hoisted(() => {
  const { reactive } = require('vue') as typeof import('vue')
  return reactive({
    generating: false,
    current: null as { id: string; messages: unknown[] } | null
  })
})

vi.mock('../../lib/runtime', () => ({ isTauriRuntime: () => false }))
vi.mock('../../lib/api', () => ({
  deleteWorkspacePath: vi.fn(async () => undefined),
  getWorkspaceGitDiff: vi.fn(async () => ({
    path: '',
    mode: 'unstaged',
    diffLines: [],
    diffStats: { adds: 0, dels: 0 }
  })),
  getWorkspaceGitStatus: apiMocks.getWorkspaceGitStatus,
  getTurnFileDiff: vi.fn(async () => ({
    path: '',
    baselineMissing: false,
    created: false,
    diffLines: [],
    diffStats: { adds: 0, dels: 0 }
  })),
  listWorkspaceDirectory: apiMocks.listWorkspaceDirectory,
  openPathWithDefaultApp: vi.fn(),
  readWorkspaceFile: apiMocks.readWorkspaceFile,
  revealInFinder: vi.fn()
}))

vi.mock('../../stores/chat', () => ({
  useChatStore: () => chatState
}))

vi.mock('../../stores/workspacePanel', () => ({
  useWorkspacePanelStore: () => ({
    pendingTurnDiff: null,
    consumePendingTurnDiff: () => null
  })
}))

import WorkspacePanel from './WorkspacePanel.vue'

const mountedApps: Array<ReturnType<typeof createApp>> = []

async function settle() {
  await Promise.resolve()
  await nextTick()
  await Promise.resolve()
  await nextTick()
}

function resetTestState() {
  chatState.generating = false
  chatState.current = null
}

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
  vi.clearAllMocks()
  resetTestState()
})

describe('WorkspacePanel refresh behavior', () => {
  it('force-refreshes Git status when entering Changes, not when already there', async () => {
    apiMocks.getWorkspaceGitStatus.mockResolvedValue({
      changes: [{ path: 'a.ts', status: 'modified', staged: false }]
    })
    const host = document.createElement('div')
    document.body.append(host)
    const app = createApp(WorkspacePanel, { workspaceRoot: '/workspace' })
    mountedApps.push(app)
    app.mount(host)
    await settle()

    const changesTab = [...host.querySelectorAll<HTMLButtonElement>('.workspace-tab-icon')]
      .find(tab => tab.getAttribute('aria-label') === '变更文件')
    expect(changesTab).toBeTruthy()

    apiMocks.getWorkspaceGitStatus.mockClear()
    apiMocks.getWorkspaceGitStatus.mockResolvedValue({
      changes: [
        { path: 'a.ts', status: 'modified', staged: false },
        { path: 'b.ts', status: 'added', staged: false }
      ]
    })
    changesTab!.click()
    await settle()
    expect(apiMocks.getWorkspaceGitStatus).toHaveBeenCalledWith('/workspace')
    expect(host.textContent).toContain('2')

    apiMocks.getWorkspaceGitStatus.mockClear()
    // Already on Changes — clicking again should not re-fetch.
    changesTab!.click()
    await settle()
    expect(apiMocks.getWorkspaceGitStatus).not.toHaveBeenCalled()
  })

  it('silently refreshes the Git badge when the window regains focus', async () => {
    const host = document.createElement('div')
    document.body.append(host)
    const app = createApp(WorkspacePanel, { workspaceRoot: '/workspace' })
    mountedApps.push(app)
    app.mount(host)
    await settle()
    apiMocks.getWorkspaceGitStatus.mockClear()

    // Bypass TTL from mount by waiting past 1.5s is slow in tests — fire focus
    // after advancing timers once the mount refresh window has elapsed.
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      vi.advanceTimersByTime(1_600)
      window.dispatchEvent(new Event('focus'))
      await settle()
      expect(apiMocks.getWorkspaceGitStatus).toHaveBeenCalledWith('/workspace')
    } finally {
      vi.useRealTimers()
    }
  })

  it('refreshes the tree and Git badge after the current turn writes a file', async () => {
    const host = document.createElement('div')
    document.body.append(host)
    chatState.current = {
      id: 'conversation-1',
      messages: [
        { id: 'turn-1', role: 'user', content: '更新文件' },
        {
          id: 'assistant-1',
          role: 'assistant',
          content: '',
          toolCalls: [{
            id: 'tool-1',
            name: 'file_write',
            status: 'success',
            arguments: '{"path":"src/new.ts"}',
            result: '{"success":true,"path":"src/new.ts"}'
          }]
        }
      ] as any
    }
    chatState.generating = true
    const app = createApp(WorkspacePanel, {
      workspaceRoot: '/workspace',
      conversationId: 'conversation-1'
    })
    mountedApps.push(app)
    app.mount(host)
    await settle()
    apiMocks.listWorkspaceDirectory.mockClear()
    apiMocks.getWorkspaceGitStatus.mockClear()

    chatState.generating = false
    await nextTick()
    await settle()

    expect(apiMocks.listWorkspaceDirectory).toHaveBeenCalledWith('/workspace')
    expect(apiMocks.getWorkspaceGitStatus).toHaveBeenCalledWith('/workspace')
  })
})

describe('WorkspacePanel terminal tab', () => {
  it('places the Terminal tab before Files and Changes', async () => {
    const host = document.createElement('div')
    document.body.append(host)
    const app = createApp(WorkspacePanel, { workspaceRoot: '/workspace' })
    mountedApps.push(app)
    app.mount(host)
    await settle()

    const tabs = [...host.querySelectorAll<HTMLButtonElement>('.workspace-tab-icon')]
    expect(tabs.slice(0, 3).map(tab => tab.getAttribute('aria-label'))).toEqual([
      '工作区文件',
      '变更文件',
      '调试终端'
    ])
  })
})

describe('WorkspacePanel Markdown references', () => {
  it('opens a relative Markdown reference in a workspace preview tab', async () => {
    apiMocks.readWorkspaceFile.mockImplementation(async (_root: string, path: string) => ({
      path,
      content: path === 'README.md'
        ? '- **用户**：[`docs/user/getting-started.md`](docs/user/getting-started.md)'
        : '# Getting started',
      sizeBytes: 64,
      truncated: false,
      binary: false
    }))
    const host = document.createElement('div')
    document.body.append(host)
    const app = createApp(WorkspacePanel, { workspaceRoot: '/workspace' })
    mountedApps.push(app)
    app.mount(host)
    await settle()

    const readmeRow = [...host.querySelectorAll<HTMLElement>('[role="button"], button')]
      .find(element => element.textContent?.includes('README.md'))
    readmeRow?.click()
    await settle()
    const reference = host.querySelector<HTMLAnchorElement>('a[href="docs/user/getting-started.md"]')
    expect(reference).not.toBeNull()
    reference?.click()
    await settle()

    expect(apiMocks.readWorkspaceFile)
      .toHaveBeenLastCalledWith('/workspace', 'docs/user/getting-started.md')
    expect(host.textContent).toContain('Getting started')
  })

  it('keeps the current preview when a Markdown reference target is unavailable', async () => {
    apiMocks.readWorkspaceFile.mockImplementation(async (_root: string, path: string) => {
      if (path === 'README.md') {
        return {
          path,
          content: '[Missing document](docs/missing.md)',
          sizeBytes: 64,
          truncated: false,
          binary: false
        }
      }
      throw new Error('File not found')
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const host = document.createElement('div')
    document.body.append(host)
    const app = createApp(WorkspacePanel, { workspaceRoot: '/workspace' })
    mountedApps.push(app)
    app.mount(host)
    await settle()

    const readmeRow = [...host.querySelectorAll<HTMLElement>('[role="button"], button')]
      .find(element => element.textContent?.includes('README.md'))
    readmeRow?.click()
    await settle()
    host.querySelector<HTMLAnchorElement>('a[href="docs/missing.md"]')?.click()
    await settle()

    expect(apiMocks.readWorkspaceFile)
      .toHaveBeenLastCalledWith('/workspace', 'docs/missing.md')
    expect(host.querySelectorAll('.workspace-preview-tab')).toHaveLength(1)
    expect(host.querySelector('.workspace-preview-tab.is-active')?.textContent).toContain('README.md')
    expect(warn).toHaveBeenCalledWith(
      '[WorkspacePanel] Markdown reference target is unavailable',
      expect.objectContaining({ path: 'docs/missing.md' })
    )
    warn.mockRestore()
  })
})
