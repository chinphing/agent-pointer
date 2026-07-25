// @vitest-environment happy-dom

import { createApp, nextTick } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'

const apiMocks = vi.hoisted(() => ({
  readWorkspaceFile: vi.fn()
}))

vi.mock('../../lib/runtime', () => ({ isTauriRuntime: () => false }))
vi.mock('../../lib/api', () => ({
  getWorkspaceGitDiff: vi.fn(async () => ({ path: '', diff: '' })),
  getWorkspaceGitStatus: vi.fn(async () => ({ changes: [] })),
  listWorkspaceDirectory: vi.fn(async () => [
    { name: 'README.md', path: 'README.md', kind: 'file', sizeBytes: 64 }
  ]),
  openPathWithDefaultApp: vi.fn(),
  readWorkspaceFile: apiMocks.readWorkspaceFile,
  revealInFinder: vi.fn()
}))

import WorkspacePanel from './WorkspacePanel.vue'

const mountedApps: Array<ReturnType<typeof createApp>> = []

async function settle() {
  await Promise.resolve()
  await nextTick()
  await Promise.resolve()
  await nextTick()
}

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
  vi.clearAllMocks()
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
})
