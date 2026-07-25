import { describe, expect, it } from 'vitest'
import {
  clampContextMenuPosition,
  clampWorkspacePanelWidth,
  GIT_INITIALIZATION_TASK,
  readWorkspacePanelWidth,
  workspaceAbsolutePath
} from './workspacePanel'

describe('workspacePanel helpers', () => {
  it('provides a Git initialization task that requires project analysis and verification', () => {
    expect(GIT_INITIALIZATION_TASK).toContain('分析项目架构和技术栈')
    expect(GIT_INITIALIZATION_TASK).toContain('构建产物、本地配置、缓存、IDE 文件及敏感文件')
    expect(GIT_INITIALIZATION_TASK).toContain('现有 .gitignore')
    expect(GIT_INITIALIZATION_TASK).toContain('先向我说明拟采用的方案')
    expect(GIT_INITIALIZATION_TASK).toContain('写入或补充 .gitignore')
    expect(GIT_INITIALIZATION_TASK).toContain('git init')
    expect(GIT_INITIALIZATION_TASK).toContain('git status')
  })

  it('clamps persisted panel widths against fixed and viewport limits', () => {
    expect(clampWorkspacePanelWidth(100, 1440)).toBe(280)
    expect(clampWorkspacePanelWidth(900, 1440)).toBe(720)
    expect(clampWorkspacePanelWidth(600, 800)).toBe(480)
    expect(readWorkspacePanelWidth('invalid', 1440)).toBe(360)
    expect(readWorkspacePanelWidth('420', 1440)).toBe(420)
  })

  it('builds platform-appropriate absolute paths from relative workspace entries', () => {
    expect(workspaceAbsolutePath('/tmp/project/', 'src/main.ts')).toBe('/tmp/project/src/main.ts')
    expect(workspaceAbsolutePath('C:\\project\\', 'src/main.ts')).toBe('C:\\project\\src\\main.ts')
  })

  it('keeps context menus inside the viewport', () => {
    expect(clampContextMenuPosition(790, 590, 800, 600)).toEqual({ left: 584, top: 372 })
    expect(clampContextMenuPosition(-5, -10, 800, 600)).toEqual({ left: 8, top: 8 })
  })
})
