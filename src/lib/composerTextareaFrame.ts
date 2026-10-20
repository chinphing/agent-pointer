/**
 * The composer textarea lives in its own document.
 *
 * On macOS, deleting the last character asks the input method for the word
 * before the caret (`Editor::stringForCandidateRequest`). That search walks
 * backwards through the document. An empty textarea has no text node, so the
 * walk leaves the control and reads the transcript — about a second on a long
 * conversation, then again when the layer tree commits editor state.
 *
 * A same-origin frame has nothing after the textarea. The walk stops there.
 * App and Web share this document; the frame is not a sandbox.
 */

export const COMPOSER_TEXTAREA_FRAME_CLASS = 'composer-textarea-frame'

/** Placeholder paint node inside the frame document. Theme updates rewrite it. */
export const COMPOSER_TEXTAREA_FRAME_PAINT_ID = 'composer-textarea-paint'

const FRAME_STYLE = `
  html, body {
    margin: 0;
    height: 100%;
    background: transparent;
    overflow: hidden;
  }
  textarea.composer-textarea {
    box-sizing: border-box;
    display: block;
    width: 100%;
    height: 36px;
    min-height: 36px;
    max-height: 250px;
    margin: 0;
    border: 0;
    outline: none;
    resize: none;
    overflow-x: hidden;
    overflow-y: auto;
    background: transparent;
    field-sizing: content;
    font-size: 15px;
    font-weight: 400;
    line-height: 20px;
    padding: 8px;
    cursor: text;
  }
`

/** Document written into the frame. No script: the parent attaches listeners. */
export function composerTextareaFrameHtml(): string {
  return `<!DOCTYPE html><html><head><meta charset="utf-8"><style>${FRAME_STYLE}</style><style id="${COMPOSER_TEXTAREA_FRAME_PAINT_ID}"></style></head><body><textarea class="composer-textarea" rows="1" autocomplete="off"></textarea></body></html>`
}

export interface ComposerTextareaFramePaint {
  color: string
  placeholderColor: string
  backgroundColor: string
  fontFamily: string
  colorScheme: 'light' | 'dark'
  /** Wide layout uses the desktop padding and line height. */
  desktop: boolean
}

/** Drop characters that would break out of the frame's style sheet. */
export function composerFrameCssValue(value: string): string {
  return value.replace(/[{}<>]/g, '').replace(/[\n\r]/g, ' ')
}

/**
 * Colors and font from the parent page. Called when the frame opens and when
 * the theme or the width breakpoint changes — never on a keystroke.
 */
export function readComposerTextareaFramePaint(
  doc: Document = document
): ComposerTextareaFramePaint {
  const body = doc.body
  const view = doc.defaultView
  const bodyStyle = view ? view.getComputedStyle(body) : null
  const color = bodyStyle?.color || 'CanvasText'
  const fontFamily = bodyStyle?.fontFamily || 'sans-serif'
  let placeholderColor = color
  let backgroundColor = 'transparent'
  if (body && view) {
    const probe = doc.createElement('span')
    probe.style.color = 'hsl(var(--muted))'
    probe.style.backgroundColor = 'hsl(var(--composer-bg))'
    probe.style.position = 'absolute'
    probe.style.left = '-10000px'
    body.appendChild(probe)
    const probeStyle = view.getComputedStyle(probe)
    placeholderColor = probeStyle.color || color
    backgroundColor = probeStyle.backgroundColor || 'transparent'
    probe.remove()
  }
  const desktop = view?.matchMedia('(min-width: 768px)').matches === true
  const colorScheme = doc.documentElement.classList.contains('dark') ? 'dark' : 'light'
  return { color, placeholderColor, backgroundColor, fontFamily, colorScheme, desktop }
}

/** Write theme colors into an already-open frame document. */
export function applyComposerTextareaFramePaint(
  frameDoc: Document,
  paint: ComposerTextareaFramePaint
): void {
  const style = frameDoc.getElementById(COMPOSER_TEXTAREA_FRAME_PAINT_ID)
  if (!style) {
    console.warn('[composer] textarea frame paint style missing')
    return
  }
  const padding = paint.desktop ? '3px 12px 8px' : '8px'
  const lineHeight = paint.desktop ? 'normal' : '20px'
  const color = composerFrameCssValue(paint.color)
  const placeholder = composerFrameCssValue(paint.placeholderColor)
  const background = composerFrameCssValue(paint.backgroundColor)
  const fontFamily = composerFrameCssValue(paint.fontFamily)
  style.textContent = `html,body{background:${background}}textarea.composer-textarea{color:${color};caret-color:${color};font-family:${fontFamily};padding:${padding};line-height:${lineHeight};background:transparent}textarea.composer-textarea::placeholder{color:${placeholder}}`
  frameDoc.documentElement.style.colorScheme = paint.colorScheme
}

/**
 * The live composer control. A parent-document `querySelector` cannot see a
 * textarea inside the frame, and `instanceof HTMLTextAreaElement` is false
 * across documents, so this checks the tag name.
 */
export function findComposerTextarea(root: ParentNode = document): HTMLTextAreaElement | null {
  const direct = root.querySelector('textarea.composer-textarea')
  if (direct && direct.tagName === 'TEXTAREA') return direct as HTMLTextAreaElement
  const frame = root.querySelector(`iframe.${COMPOSER_TEXTAREA_FRAME_CLASS}`)
  if (!frame || !('contentDocument' in frame)) return null
  const nested = (frame as HTMLIFrameElement).contentDocument?.querySelector(
    'textarea.composer-textarea'
  )
  if (!nested || nested.tagName !== 'TEXTAREA') return null
  return nested as HTMLTextAreaElement
}
