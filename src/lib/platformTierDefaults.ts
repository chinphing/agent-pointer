import type {
  ComputerPipelineLlmSettings,
  ComputerTierLlmConfig,
  ModelRuntimeOverrides,
  PlatformProviderModelTemplate,
  PlatformProviderTemplate,
  PlatformSettings,
  ProviderConfig
} from '../types/chat'
import { parseThinkingIntensity, thinkingPatchFromProviderModel } from './thinkingIntensity'

export type PlatformTierRef = { providerId: string; model: string }

export function emptyTierConfig(): ComputerTierLlmConfig {
  return { providerId: '', model: '' }
}

export function platformModelName(entry: string | PlatformProviderModelTemplate | unknown): string {
  if (typeof entry === 'string') return entry.trim()
  if (entry && typeof entry === 'object' && 'name' in entry) {
    return String((entry as PlatformProviderModelTemplate).name ?? '').trim()
  }
  return ''
}

export function platformModelNames(models: unknown): string[] {
  if (!Array.isArray(models)) return []
  const names: string[] = []
  const seen = new Set<string>()
  for (const entry of models) {
    const name = platformModelName(entry)
    if (!name || seen.has(name)) continue
    seen.add(name)
    names.push(name)
  }
  return names
}

function modelEntryOverrides(entry: unknown): ModelRuntimeOverrides {
  if (!entry || typeof entry !== 'object' || !('name' in entry)) return {}
  const raw = entry as PlatformProviderModelTemplate
  const over: ModelRuntimeOverrides = {}
  if (raw.reasoningInMessages !== undefined) over.reasoningInMessages = raw.reasoningInMessages
  if (raw.temperature !== undefined) over.temperature = raw.temperature
  if (raw.topP !== undefined) over.topP = raw.topP
  if (raw.maxTokens !== undefined) over.maxTokens = raw.maxTokens
  if (raw.enableThinking !== undefined) over.enableThinking = raw.enableThinking
  if (raw.thinkingBudget !== undefined) over.thinkingBudget = raw.thinkingBudget
  if (raw.reasoningEffort === 'high' || raw.reasoningEffort === 'max') {
    over.reasoningEffort = raw.reasoningEffort
  }
  const intensity = parseThinkingIntensity(raw.thinkingIntensity)
  if (intensity) over.thinkingIntensity = intensity
  if (raw.supportsVision !== undefined) over.supportsVision = raw.supportsVision
  if (raw.supportsAudio !== undefined) over.supportsAudio = raw.supportsAudio
  if (raw.canGenerateImage !== undefined) over.canGenerateImage = raw.canGenerateImage
  if (raw.canGenerateVideo !== undefined) over.canGenerateVideo = raw.canGenerateVideo
  return over
}

export function platformModelConfigs(
  models: unknown
): Record<string, ModelRuntimeOverrides> {
  if (!Array.isArray(models)) return {}
  const configs: Record<string, ModelRuntimeOverrides> = {}
  for (const entry of models) {
    const name = platformModelName(entry)
    if (!name) continue
    const over = modelEntryOverrides(entry)
    if (Object.keys(over).length) configs[name] = over
  }
  return configs
}

/** Merge per-model overrides. Platform capability flags win when set. */
export function mergePlatformModelConfigs(
  platform: Record<string, ModelRuntimeOverrides> | undefined,
  overlay: Record<string, ModelRuntimeOverrides> | undefined
): Record<string, ModelRuntimeOverrides> {
  const plat = platform ?? {}
  const over = overlay ?? {}
  const keys = new Set([...Object.keys(plat), ...Object.keys(over)])
  const out: Record<string, ModelRuntimeOverrides> = {}
  for (const key of keys) {
    const a = plat[key] ?? {}
    const b = over[key] ?? {}
    const merged: ModelRuntimeOverrides = { ...a, ...b }
    if (a.supportsVision !== undefined) merged.supportsVision = a.supportsVision
    if (a.supportsAudio !== undefined) merged.supportsAudio = a.supportsAudio
    if (a.canGenerateImage !== undefined) merged.canGenerateImage = a.canGenerateImage
    if (a.canGenerateVideo !== undefined) merged.canGenerateVideo = a.canGenerateVideo
    if (Object.keys(merged).length) out[key] = merged
  }
  return out
}

/** Accept both the array form from the API and a legacy id-keyed record. */
export function normalizePlatformProviderTemplates(
  raw?: PlatformSettings['platformProviders']
): ProviderConfig[] {
  if (!raw) return []
  const entries: Array<{ id: string; tpl: PlatformProviderTemplate }> = Array.isArray(raw)
    ? raw
        .map(tpl => ({ id: String(tpl.id ?? '').trim(), tpl }))
        .filter(item => item.id)
    : Object.entries(raw).map(([id, tpl]) => ({ id, tpl }))
  return entries.map(({ id, tpl }) => ({
    id,
    name: tpl.name ?? id,
    baseUrl: tpl.baseUrl ?? '',
    apiKey: '',
    models: platformModelNames(tpl.models),
    reasoningInMessages: tpl.reasoningInMessages,
    enableThinking: tpl.enableThinking,
    thinkingBudget: tpl.thinkingBudget,
    reasoningEffort:
      tpl.reasoningEffort === 'high' || tpl.reasoningEffort === 'max'
        ? tpl.reasoningEffort
        : undefined,
    thinkingIntensity: parseThinkingIntensity(tpl.thinkingIntensity) || undefined,
    temperature: tpl.temperature,
    topP: tpl.topP,
    maxTokens: tpl.maxTokens,
    modelConfigs: platformModelConfigs(tpl.models)
  }))
}

export function readTierModelRef(raw: unknown): PlatformTierRef | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const obj = raw as Record<string, unknown>
  const providerId = String(obj.providerId ?? obj.provider_id ?? '').trim()
  const model = String(obj.model ?? '').trim()
  if (!providerId || !model) return undefined
  return { providerId, model }
}

function tierDefaultsObject(tierDefaults: unknown): Record<string, unknown> | undefined {
  if (!tierDefaults || typeof tierDefaults !== 'object' || Array.isArray(tierDefaults)) {
    return undefined
  }
  return tierDefaults as Record<string, unknown>
}

function nestedMap(
  root: Record<string, unknown> | undefined,
  key: string
): Record<string, unknown> | undefined {
  const value = root?.[key]
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  return value as Record<string, unknown>
}

export function withInheritedThinking(
  ref: PlatformTierRef,
  providers: ProviderConfig[]
): ComputerTierLlmConfig {
  return {
    providerId: ref.providerId,
    model: ref.model,
    ...thinkingPatchFromProviderModel(providers, ref.providerId, ref.model)
  }
}

export function platformAgentModeDefault(
  tierDefaults: unknown,
  agentId: string,
  mode: string
): PlatformTierRef | undefined {
  const modes = nestedMap(nestedMap(tierDefaultsObject(tierDefaults), 'agentModeLlm'), agentId)
  return readTierModelRef(modes?.[mode])
}

export function platformMediaModeDefault(
  tierDefaults: unknown,
  kind: string,
  mode: string
): PlatformTierRef | undefined {
  const modes = nestedMap(nestedMap(tierDefaultsObject(tierDefaults), 'mediaModeLlm'), kind)
  return readTierModelRef(modes?.[mode])
}

export function platformComputerTierDefault(
  tierDefaults: unknown,
  tier: string
): PlatformTierRef | undefined {
  const tiers = nestedMap(tierDefaultsObject(tierDefaults), 'computerTierLlm')
  return readTierModelRef(tiers?.[tier])
}

export function platformMediaGenerationDefault(
  tierDefaults: unknown,
  kind: 'image' | 'video'
): PlatformTierRef | undefined {
  const generation = nestedMap(tierDefaultsObject(tierDefaults), 'mediaGeneration')
  return readTierModelRef(generation?.[kind])
}

export function platformComputerPipelineDefault(
  tierDefaults: unknown
): ComputerPipelineLlmSettings {
  const pipeline = nestedMap(tierDefaultsObject(tierDefaults), 'computerPipelineLlm')
  const out: ComputerPipelineLlmSettings = {}
  const decision = readTierModelRef(pipeline?.decision)
  if (decision) {
    out.decision = decision.model
    out.decisionProviderId = decision.providerId
  }
  const position = readTierModelRef(pipeline?.position)
  if (position) {
    out.position = position.model
    out.positionProviderId = position.providerId
  }
  const verify = readTierModelRef(pipeline?.verify)
  if (verify) {
    out.verify = verify.model
    out.verifyProviderId = verify.providerId
  }
  return out
}

export function isSameTierRef(
  config: { providerId?: string; model?: string },
  fallback?: PlatformTierRef
): boolean {
  if (!fallback) return false
  return (
    (config.providerId ?? '').trim() === fallback.providerId &&
    (config.model ?? '').trim() === fallback.model
  )
}
