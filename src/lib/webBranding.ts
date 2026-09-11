/** Default composer prompt when no server / Vite override is present. */
export const DEFAULT_COMPOSER_PLACEHOLDER = '告诉我你想做什么'

/** Default turn-elapsed chip prefix (`工作 N m SS s` / `工作耗时未知`). */
export const DEFAULT_TURN_ELAPSED_PREFIX = '工作'

const COMPOSER_PLACEHOLDER_META = 'pointer-composer-placeholder'
const WELCOME_TIP_TITLE_META = 'pointer-welcome-tip-title'
const WELCOME_TIP_BODY_META = 'pointer-welcome-tip-body'
const TURN_ELAPSED_ACTIVE_META = 'pointer-turn-elapsed-active'
const TURN_ELAPSED_DONE_META = 'pointer-turn-elapsed-done'

function readMeta(name: string): string | null {
  if (typeof document === 'undefined') return null
  const meta = document
    .querySelector(`meta[name="${name}"]`)
    ?.getAttribute('content')
    ?.trim()
  return meta || null
}

function readViteString(key: string): string | null {
  const value = (import.meta.env as Record<string, unknown>)[key]
  if (typeof value === 'string' && value.trim()) return value.trim()
  return null
}

/**
 * Composer empty-state placeholder for the default (logged-in + key) case.
 * Priority: `VITE_COMPOSER_PLACEHOLDER` → `<meta name="pointer-composer-placeholder">`
 * (injected by pointer-server from `[server].composer_placeholder`) → default.
 */
export function resolveComposerPlaceholder(): string {
  return (
    readViteString('VITE_COMPOSER_PLACEHOLDER')
    ?? readMeta(COMPOSER_PLACEHOLDER_META)
    ?? DEFAULT_COMPOSER_PLACEHOLDER
  )
}

export type WelcomeTip = {
  title: string | null
  body: string | null
}

/**
 * Brand-new empty conversation tip (Web branding).
 * Priority: Vite → server-injected meta. Missing both → no tip.
 */
export function resolveWelcomeTip(): WelcomeTip | null {
  const title =
    readViteString('VITE_WELCOME_TIP_TITLE')
    ?? readMeta(WELCOME_TIP_TITLE_META)
  const body =
    readViteString('VITE_WELCOME_TIP_BODY')
    ?? readMeta(WELCOME_TIP_BODY_META)
  if (!title && !body) return null
  return { title, body }
}

export type TurnElapsedPhase = 'active' | 'done'

/**
 * Turn elapsed chip prefix. Default `工作`.
 * Priority: Vite → server meta → default.
 */
export function resolveTurnElapsedPrefix(phase: TurnElapsedPhase): string {
  if (phase === 'active') {
    return (
      readViteString('VITE_TURN_ELAPSED_ACTIVE')
      ?? readMeta(TURN_ELAPSED_ACTIVE_META)
      ?? DEFAULT_TURN_ELAPSED_PREFIX
    )
  }
  return (
    readViteString('VITE_TURN_ELAPSED_DONE')
    ?? readMeta(TURN_ELAPSED_DONE_META)
    ?? DEFAULT_TURN_ELAPSED_PREFIX
  )
}
