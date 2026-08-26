import { workspaceRichPreviewKind } from './workspacePreviewMode'

/**
 * Scripts run; unique origin so the page cannot read the app.
 * Do not add allow-same-origin together with allow-scripts.
 */
export const HTML_PREVIEW_SANDBOX = 'allow-scripts allow-modals'

/** Set on the workspace panel while dragging its width; CSS freezes iframe layout. */
export const WORKSPACE_IFRAME_FREEZE_WIDTH_VAR = '--workspace-iframe-freeze-width'

const HTML_PREVIEW_HEAD = '<meta charset="utf-8">'

export function isHtmlPreviewPath(path: string): boolean {
  return workspaceRichPreviewKind(path) === 'html'
}

/** srcdoc for the sandboxed HTML preview iframe. Inline JS is kept. */
export function buildHtmlPreviewSrcdoc(raw: string): string {
  if (/<head\b/i.test(raw)) {
    return raw.replace(/<head\b([^>]*)>/i, `<head$1>${HTML_PREVIEW_HEAD}`)
  }
  if (/<html\b/i.test(raw)) {
    return raw.replace(/<html\b([^>]*)>/i, `<html$1><head>${HTML_PREVIEW_HEAD}</head>`)
  }
  return `<!DOCTYPE html><html><head>${HTML_PREVIEW_HEAD}</head><body>${raw}</body></html>`
}

/**
 * Width of the first on-screen iframe under `root`.
 * Hidden preview tabs (v-show) report 0 and are skipped.
 */
export function visibleIframeOffsetWidth(root: ParentNode | null): number | null {
  if (!root) return null
  for (const node of root.querySelectorAll('iframe')) {
    const width = (node as HTMLElement).offsetWidth
    if (width > 0) return width
  }
  return null
}
