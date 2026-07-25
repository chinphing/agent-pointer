export const GIT_INITIALIZATION_TASK = '请为当前工作区初始化 Git 仓库。先分析项目架构和技术栈，识别构建产物、本地配置、缓存、IDE 文件及敏感文件；检查现有 .gitignore，并结合分析结果决定合适的忽略模板。先向我说明拟采用的方案，再写入或补充 .gitignore，最后执行 git init，并运行 git status 验证初始化结果。'

export const WORKSPACE_PANEL_MIN_WIDTH = 280
export const WORKSPACE_PANEL_DEFAULT_WIDTH = 360

export function clampWorkspacePanelWidth(
  width: number,
  viewportWidth = typeof window === 'undefined' ? 1440 : window.innerWidth
): number {
  const responsiveMax = Math.max(WORKSPACE_PANEL_MIN_WIDTH, viewportWidth - 320)
  return Math.round(Math.min(Math.max(width, WORKSPACE_PANEL_MIN_WIDTH), responsiveMax))
}

export function readWorkspacePanelWidth(value: string | null, viewportWidth?: number): number {
  const parsed = value === null ? Number.NaN : Number(value)
  return clampWorkspacePanelWidth(Number.isFinite(parsed) ? parsed : WORKSPACE_PANEL_DEFAULT_WIDTH, viewportWidth)
}

export function workspaceAbsolutePath(workspaceRoot: string, relativePath: string): string {
  const root = workspaceRoot.replace(/[\\/]+$/, '')
  const relative = relativePath.replace(/^[\\/]+/, '')
  if (!relative) return root
  const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  return `${root}${separator}${relative.replace(/[\\/]/g, separator)}`
}

export function clampContextMenuPosition(
  clientX: number,
  clientY: number,
  viewportWidth: number,
  viewportHeight: number,
  menuWidth = 208,
  menuHeight = 220
): { left: number; top: number } {
  return {
    left: Math.max(8, Math.min(clientX, viewportWidth - menuWidth - 8)),
    top: Math.max(8, Math.min(clientY, viewportHeight - menuHeight - 8))
  }
}
