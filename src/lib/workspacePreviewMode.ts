export type WorkspacePreviewViewMode = 'source' | 'preview'

/** Structured (non-source) text previews. Image / PDF / binary are a separate media path. */
export type WorkspaceRichPreviewKind = 'markdown' | 'json' | 'html'

export type WorkspaceTextPreviewSurface = WorkspaceRichPreviewKind | 'source'

export type WorkspaceRichPreviewDef = {
  kind: WorkspaceRichPreviewKind
  extensions: readonly string[]
  /**
   * True: 原文/预览 switch is always shown.
   * False: only when the kind-specific renderer can build (e.g. JSON.parse).
   */
  previewAlwaysReady: boolean
}

export const WORKSPACE_RICH_PREVIEW_DEFS: readonly WorkspaceRichPreviewDef[] = [
  { kind: 'markdown', extensions: ['md'], previewAlwaysReady: true },
  { kind: 'json', extensions: ['json', 'jsonc'], previewAlwaysReady: false },
  { kind: 'html', extensions: ['html', 'htm'], previewAlwaysReady: true }
]

const RICH_KIND_BY_EXT = new Map<string, WorkspaceRichPreviewKind>()
const DEF_BY_KIND = new Map<WorkspaceRichPreviewKind, WorkspaceRichPreviewDef>()
for (const def of WORKSPACE_RICH_PREVIEW_DEFS) {
  DEF_BY_KIND.set(def.kind, def)
  for (const ext of def.extensions) {
    if (RICH_KIND_BY_EXT.has(ext)) {
      throw new Error(`[workspacePreviewMode] Duplicate preview extension: ${ext}`)
    }
    RICH_KIND_BY_EXT.set(ext, def.kind)
  }
}

/** First usable lowercase extension from relative / preview / absolute paths. */
export function workspaceFileExtension(
  ...paths: Array<string | null | undefined>
): string {
  for (const path of paths) {
    if (!path) continue
    const base = path.split(/[\\/]/).pop() ?? ''
    const dot = base.lastIndexOf('.')
    if (dot <= 0 || dot === base.length - 1) continue
    return base.slice(dot + 1).toLowerCase()
  }
  return ''
}

export function workspaceRichPreviewKindFromExt(
  ext: string
): WorkspaceRichPreviewKind | null {
  return RICH_KIND_BY_EXT.get(ext) ?? null
}

export function workspaceRichPreviewKind(
  ...paths: Array<string | null | undefined>
): WorkspaceRichPreviewKind | null {
  return workspaceRichPreviewKindFromExt(workspaceFileExtension(...paths))
}

export function workspaceRichPreviewAlwaysReady(
  kind: WorkspaceRichPreviewKind
): boolean {
  return DEF_BY_KIND.get(kind)?.previewAlwaysReady ?? false
}

/**
 * Which text surface to render. Media / binary must be handled before this.
 * `contentReady` is true for always-ready kinds, or after the kind-specific parse.
 */
export function workspaceTextPreviewSurface(
  kind: WorkspaceRichPreviewKind | null,
  viewMode: WorkspacePreviewViewMode,
  contentReady: boolean
): WorkspaceTextPreviewSurface {
  if (kind && viewMode === 'preview' && contentReady) return kind
  return 'source'
}
