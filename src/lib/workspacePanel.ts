export const GIT_INITIALIZATION_TASK = '请为当前工作区初始化 Git 仓库。先分析项目架构和技术栈，识别构建产物、本地配置、缓存、IDE 文件及敏感文件；检查现有 .gitignore，并结合分析结果决定合适的忽略模板。先向我说明拟采用的方案，再写入或补充 .gitignore，最后执行 git init，并运行 git status 验证初始化结果。'

export const WORKSPACE_PANEL_MIN_WIDTH = 280
export const WORKSPACE_PANEL_DEFAULT_WIDTH = 360
// Keep the fixed cap deliberately above normal desktop widths. The actual limit
// is viewport width minus the left-side chat/navigation area below.
export const WORKSPACE_PANEL_MAX_WIDTH = 2_000

export function clampWorkspacePanelWidth(
  width: number,
  viewportWidth = typeof window === 'undefined' ? 1440 : window.innerWidth
): number {
  const responsiveMax = Math.max(
    WORKSPACE_PANEL_MIN_WIDTH,
    Math.min(WORKSPACE_PANEL_MAX_WIDTH, viewportWidth - 320)
  )
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

export type WorkspaceMarkdownReference =
  | { kind: 'external'; url: string }
  | { kind: 'workspace'; path: string }
  | { kind: 'local'; path: string }
  | { kind: 'unsupported'; label: string }

function decodeReferencePath(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}

function isAbsoluteFilesystemPath(path: string): boolean {
  return path.startsWith('/') || path.startsWith('\\\\') || /^[A-Za-z]:[\\/]/.test(path)
}

function workspaceRelativePath(workspaceRoot: string, absolutePath: string): string | null {
  const normalizedRoot = workspaceRoot.replace(/\\/g, '/').replace(/\/+$/, '')
  const normalizedPath = absolutePath.replace(/\\/g, '/')
  const caseInsensitive = /^[A-Za-z]:\//.test(normalizedRoot)
  const rootForCompare = caseInsensitive ? normalizedRoot.toLowerCase() : normalizedRoot
  const pathForCompare = caseInsensitive ? normalizedPath.toLowerCase() : normalizedPath
  if (pathForCompare === rootForCompare) return ''
  if (!pathForCompare.startsWith(`${rootForCompare}/`)) return null
  return normalizedPath.slice(normalizedRoot.length + 1)
}

export function resolveWorkspaceMarkdownReference(
  workspaceRoot: string,
  sourcePath: string,
  href: string
): WorkspaceMarkdownReference {
  const trimmed = href.trim()
  if (!trimmed) return { kind: 'unsupported', label: href }
  const decodedPathPart = decodeReferencePath(trimmed.split('#', 1)[0]!.split('?', 1)[0]!)

  // A Windows drive path is parsed as a URL with a one-letter protocol, so
  // filesystem absolutes must be classified before URL handling.
  if (isAbsoluteFilesystemPath(decodedPathPart)) {
    const relative = workspaceRelativePath(workspaceRoot, decodedPathPart)
    return relative === null
      ? { kind: 'local', path: decodedPathPart }
      : { kind: 'workspace', path: relative }
  }

  try {
    const url = new URL(trimmed)
    if (url.protocol === 'http:' || url.protocol === 'https:') {
      return { kind: 'external', url: trimmed }
    }
    if (url.protocol === 'file:') {
      let localPath = decodeReferencePath(url.pathname)
      if (/^\/[A-Za-z]:\//.test(localPath)) localPath = localPath.slice(1)
      const relative = workspaceRelativePath(workspaceRoot, localPath)
      return relative === null
        ? { kind: 'local', path: localPath }
        : { kind: 'workspace', path: relative }
    }
    return { kind: 'unsupported', label: trimmed }
  } catch {
    // Relative and absolute filesystem references are handled below.
  }

  const pathPart = decodedPathPart
  if (!pathPart) return { kind: 'unsupported', label: trimmed }

  const sourceDirectory = sourcePath.replace(/\\/g, '/').split('/').slice(0, -1)
  const parts = [...sourceDirectory]
  let escapedWorkspace = false
  for (const part of pathPart.replace(/\\/g, '/').split('/')) {
    if (!part || part === '.') continue
    if (part === '..') {
      if (parts.length > 0) parts.pop()
      else escapedWorkspace = true
      continue
    }
    parts.push(part)
  }
  if (!escapedWorkspace) return { kind: 'workspace', path: parts.join('/') }

  const unresolved = [...sourceDirectory, pathPart].join('/')
  return { kind: 'local', path: workspaceAbsolutePath(workspaceRoot, unresolved) }
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
