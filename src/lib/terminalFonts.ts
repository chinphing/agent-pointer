/**
 * xterm font stacks: system monospace first, then the OS default Chinese UI font.
 *
 * Do not put proportional CJK faces first — that stretches Latin cell metrics.
 * Do not register `local()` FontFace aliases that can shadow real system fonts.
 */

import { detectDesktopOs, type DesktopOs } from './desktopOs'

/** macOS: Menlo/SF Mono + system 苹方. */
export const TERMINAL_FONT_FAMILY_MAC =
  'ui-monospace, SFMono-Regular, Menlo, Monaco, "PingFang SC", monospace'

/** Windows: Consolas + 微软雅黑. */
export const TERMINAL_FONT_FAMILY_WIN =
  'Consolas, "Cascadia Mono", "Courier New", "Microsoft YaHei UI", "Microsoft YaHei", monospace'

/** Linux: system ui-monospace + common CJK UI fonts from the distro. */
export const TERMINAL_FONT_FAMILY_LINUX =
  'ui-monospace, "DejaVu Sans Mono", "Noto Sans Mono CJK SC", "Noto Sans CJK SC", "WenQuanYi Micro Hei", monospace'

/** @deprecated Prefer {@link terminalFontFamily}. */
export const TERMINAL_CJK_FONT_FAMILY = TERMINAL_FONT_FAMILY_MAC

export type TerminalFontHost = {
  os: DesktopOs
}

/** Detect OS (injectable for tests). */
export function detectTerminalFontHost(
  nav?: Pick<Navigator, 'userAgent' | 'platform'>
): TerminalFontHost {
  return { os: detectDesktopOs(nav) }
}

/** CSS `font-family` for the current (or injected) host. */
export function terminalFontFamily(host?: TerminalFontHost): string {
  const os = (host ?? detectTerminalFontHost()).os
  switch (os) {
    case 'windows':
      return TERMINAL_FONT_FAMILY_WIN
    case 'linux':
      return TERMINAL_FONT_FAMILY_LINUX
    case 'macos':
    default:
      return TERMINAL_FONT_FAMILY_MAC
  }
}

/** Resolve stack before xterm opens (no FontFace side effects). */
export async function ensureTerminalFontsReady(host?: TerminalFontHost): Promise<string> {
  return terminalFontFamily(host)
}
