/**
 * Parse a `providerId:model` option value from model select dropdowns.
 * Returns `{ providerId, model }` when both halves are non-empty, else null.
 *
 * Using the explicit provider prefix prevents ambiguity when two providers
 * ship models with the same id — previously `find(model)` picked the first
 * provider, silently mis-selecting same-named models.
 */
export function splitProviderModelValue(
  value: string
): { providerId: string; model: string } | null {
  const i = value.indexOf(':')
  if (i <= 0 || i >= value.length - 1) return null
  const providerId = value.slice(0, i).trim()
  const model = value.slice(i + 1).trim()
  return providerId && model ? { providerId, model } : null
}
