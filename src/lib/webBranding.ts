import { isTauriRuntime } from './runtime'
import { t } from '../i18n'

/** Default composer prompt when no server / Vite override is present. */
export function defaultComposerPlaceholder(): string {
  return t('chat.composer.defaultPlaceholder')
}

/** @deprecated Prefer defaultComposerPlaceholder() so copy follows the active locale. */
export const DEFAULT_COMPOSER_PLACEHOLDER = defaultComposerPlaceholder()

/** Default turn-elapsed chip prefix (`Working N m SS s` / unknown). */
export function defaultTurnElapsedPrefix(): string {
  return t('chat.turnElapsed.prefix')
}

/** @deprecated Prefer defaultTurnElapsedPrefix() so copy follows the active locale. */
export const DEFAULT_TURN_ELAPSED_PREFIX = defaultTurnElapsedPrefix()

/** Default sidebar / top-bar product name. */
export const DEFAULT_BRAND_NAME = 'Pointer'

/** Default brand icon for top-left and bottom-left (served from `public/`). */
export const DEFAULT_BRAND_ICON = '/app-icon.png'

const COMPOSER_PLACEHOLDER_META = 'pointer-composer-placeholder'
const WELCOME_TIP_TITLE_META = 'pointer-welcome-tip-title'
const WELCOME_TIP_BODY_META = 'pointer-welcome-tip-body'
const TURN_ELAPSED_ACTIVE_META = 'pointer-turn-elapsed-active'
const TURN_ELAPSED_DONE_META = 'pointer-turn-elapsed-done'
const BRAND_NAME_META = 'pointer-brand-name'
const BRAND_ICON_META = 'pointer-brand-icon'
const DESKTOP_SNAPSHOT_META = 'pointer-desktop-snapshot'

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

function parseBoolFlag(raw: string | null): boolean | null {
  if (raw == null) return null
  const v = raw.trim().toLowerCase()
  if (!v) return null
  if (v === '1' || v === 'true' || v === 'yes' || v === 'on') return true
  if (v === '0' || v === 'false' || v === 'no' || v === 'off') return false
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
    ?? defaultComposerPlaceholder()
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
      ?? defaultTurnElapsedPrefix()
    )
  }
  return (
    readViteString('VITE_TURN_ELAPSED_DONE')
    ?? readMeta(TURN_ELAPSED_DONE_META)
    ?? defaultTurnElapsedPrefix()
  )
}

/**
 * Sidebar / top-bar product name (`Pointer`).
 * Priority: Vite → server meta → default.
 */
export function resolveBrandName(): string {
  return (
    readViteString('VITE_BRAND_NAME')
    ?? readMeta(BRAND_NAME_META)
    ?? DEFAULT_BRAND_NAME
  )
}

/**
 * Brand icon for top-left and bottom-left (same logo).
 * Priority: Vite → server meta → `/app-icon.png`.
 */
export function resolveBrandIcon(): string {
  return (
    readViteString('VITE_BRAND_ICON')
    ?? readMeta(BRAND_ICON_META)
    ?? DEFAULT_BRAND_ICON
  )
}


/**
 * Whether the desktop snapshot button should show.
 * The desktop app never shows it. Web / standalone follow
 * `pointer-desktop-snapshot` (`1`/`0`, headless hosts get `0`); unset → hide.
 */
export function resolveDesktopSnapshotEnabled(): boolean {
  if (isTauriRuntime()) return false
  const fromVite = parseBoolFlag(readViteString('VITE_DESKTOP_SNAPSHOT_ENABLED'))
  if (fromVite != null) return fromVite
  const fromMeta = parseBoolFlag(readMeta(DESKTOP_SNAPSHOT_META))
  if (fromMeta != null) return fromMeta
  return false
}
