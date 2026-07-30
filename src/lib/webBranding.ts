/** Default composer prompt when no server / Vite override is present. */
export const DEFAULT_COMPOSER_PLACEHOLDER = '告诉我你想做什么'

const COMPOSER_PLACEHOLDER_META = 'pointer-composer-placeholder'

/**
 * Composer empty-state placeholder for the default (logged-in + key) case.
 * Priority: `VITE_COMPOSER_PLACEHOLDER` → `<meta name="pointer-composer-placeholder">`
 * (injected by pointer-server from `[server].composer_placeholder`) → default.
 */
export function resolveComposerPlaceholder(): string {
  const fromVite = import.meta.env.VITE_COMPOSER_PLACEHOLDER
  if (typeof fromVite === 'string' && fromVite.trim()) {
    return fromVite.trim()
  }
  if (typeof document !== 'undefined') {
    const meta = document
      .querySelector(`meta[name="${COMPOSER_PLACEHOLDER_META}"]`)
      ?.getAttribute('content')
      ?.trim()
    if (meta) return meta
  }
  return DEFAULT_COMPOSER_PLACEHOLDER
}
