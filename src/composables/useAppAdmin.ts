import { computed } from 'vue'
import { usePlatformAuthStore } from '../stores/platformAuth'
import { useSettingsStore } from '../stores/settings'
import { isTauriRuntime } from '../lib/runtime'

/**
 * Whether this surface may open the admin-gated UI (settings view, workspace panel).
 *
 * The desktop client always may. A single local operator owns that machine, and every
 * section it exposes writes to *user* settings — providers and tiers go through
 * `update_user_settings`, channels through the local `channels_config.json`. There is
 * no platform to administer, so gating it only locked the owner out of their own app.
 *
 * Platform deployments keep the gate: there a shared server holds the IM gateway and
 * the debug switches, and those stay with the platform admin.
 */
export function useIsAppAdmin() {
  const platformAuth = usePlatformAuthStore()
  const settings = useSettingsStore()
  return computed(
    () => isTauriRuntime() || platformAuth.isPlatformAdmin || settings.isPlatformAdmin
  )
}

/**
 * Whether the settings dialog may show the admin-only debug section.
 *
 * Same policy as the backend `effective_settings_view().is_platform_admin`: the
 * session ticket flag, or the backend's verdict for this session. A standalone
 * deployment is **not** an admin signal by itself — an SSO user who signed in to a
 * standalone server is an ordinary member there, and the backend already reports
 * the desktop local operator (no web session) as an admin.
 */
export function shouldShowAdminDebugSection(input: {
  settingsAdmin: boolean
  sessionAdmin: boolean
}): boolean {
  return input.settingsAdmin || input.sessionAdmin
}
