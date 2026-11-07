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
