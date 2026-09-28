/** Minimal shape needed to decide whether the composer has a usable key. */
export type ProviderKeyView = {
  hasKey?: boolean
  providers?: Array<{ apiKey?: string } | null | undefined> | null
}

/**
 * Whether any configured provider carries an API key.
 *
 * The merged `hasKey` only reflects the *default* provider, while a turn resolves
 * its provider from the selected agent + tier (`session_model.rs`
 * `apply_agent_model_defaults`). Gating the composer on `hasKey` alone therefore
 * blocks turns that would have run on another provider's key. The backend still
 * reports the precise failure (`尚未配置 API Key（{provider}）`) when the resolution
 * really lands on a keyless provider.
 */
export function hasAnyProviderKey(settings: ProviderKeyView): boolean {
  if (settings.hasKey) return true
  return (settings.providers ?? []).some(provider => Boolean(provider?.apiKey?.trim()))
}
