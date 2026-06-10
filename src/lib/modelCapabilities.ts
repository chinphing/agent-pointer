import type { ModelRuntimeOverrides, ProviderConfig } from '../types/chat'

export interface ModelCapabilityFlags {
  supportsVision?: boolean
  canGenerateImage?: boolean
  canGenerateVideo?: boolean
}

/** Heuristic defaults when provider modelConfigs has no explicit capability flags. */
export function inferModelCapabilities(modelId: string): ModelCapabilityFlags {
  const m = modelId.trim().toLowerCase()
  if (!m) return {}

  const flags: ModelCapabilityFlags = {}

  if (
    m.includes('-vl-') ||
    m.includes('omni') ||
    m.startsWith('qwen3.5-') ||
    m.startsWith('qwen3.6-') ||
    m.startsWith('qwen3.7-') ||
    m.includes('qwen-vl') ||
    m.startsWith('gpt-4o') ||
    m.startsWith('gpt-4.1') ||
    m.includes('claude-3') ||
    m.includes('claude-sonnet-4') ||
    m.includes('claude-opus-4') ||
    m.startsWith('deepseek-v4-')
  ) {
    flags.supportsVision = true
  }

  if (
    m.includes('image') ||
    m.includes('seedream') ||
    m.includes('wan2.') && m.includes('image')
  ) {
    flags.canGenerateImage = true
  }

  if (
    m.includes('t2v') ||
    m.includes('seedance') ||
    m.includes('happyhorse') ||
    (m.includes('wan2.') && !m.includes('image'))
  ) {
    flags.canGenerateVideo = true
  }

  return flags
}

function modelOverride(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): ModelRuntimeOverrides | undefined {
  const p = providers.find(x => x.id === providerId)
  return p?.modelConfigs?.[model.trim()]
}

export function resolvedModelCapabilities(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): Required<ModelCapabilityFlags> {
  const over = modelOverride(providers, providerId, model)
  const inferred = inferModelCapabilities(model)
  return {
    supportsVision: over?.supportsVision ?? inferred.supportsVision ?? false,
    canGenerateImage: over?.canGenerateImage ?? inferred.canGenerateImage ?? false,
    canGenerateVideo: over?.canGenerateVideo ?? inferred.canGenerateVideo ?? false
  }
}

export function modelSupportsVision(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): boolean {
  return resolvedModelCapabilities(providers, providerId, model).supportsVision
}

export function modelCanGenerateImage(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): boolean {
  return resolvedModelCapabilities(providers, providerId, model).canGenerateImage
}

export function modelCanGenerateVideo(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): boolean {
  return resolvedModelCapabilities(providers, providerId, model).canGenerateVideo
}

/** Seed default capability flags into provider modelConfigs for known models (no overwrite). */
export function seedProviderModelCapabilities(provider: ProviderConfig): ProviderConfig {
  const configs = { ...(provider.modelConfigs ?? {}) }
  for (const model of provider.models ?? []) {
    const inferred = inferModelCapabilities(model)
    const hasAny =
      inferred.supportsVision || inferred.canGenerateImage || inferred.canGenerateVideo
    if (!hasAny) continue
    const prev = configs[model] ?? {}
    const next: ModelRuntimeOverrides = { ...prev }
    if (next.supportsVision === undefined && inferred.supportsVision !== undefined) {
      next.supportsVision = inferred.supportsVision
    }
    if (next.canGenerateImage === undefined && inferred.canGenerateImage !== undefined) {
      next.canGenerateImage = inferred.canGenerateImage
    }
    if (next.canGenerateVideo === undefined && inferred.canGenerateVideo !== undefined) {
      next.canGenerateVideo = inferred.canGenerateVideo
    }
    if (Object.keys(next).length) configs[model] = next
  }
  return { ...provider, modelConfigs: configs }
}

export const QWEN_IMAGE_GENERATION_MODELS = [
  'wan2.7-image-pro',
  'qwen-image-2.0-pro'
] as const

/** Mainstream Qwen video models (HappyHorse series). */
export const QWEN_VIDEO_GENERATION_MODELS = [
  'happyhorse-1.0-t2v',
  'happyhorse-1.0-i2v'
] as const

export const QWEN_GENERATION_MODELS = [
  ...QWEN_IMAGE_GENERATION_MODELS,
  ...QWEN_VIDEO_GENERATION_MODELS
] as const

export const DOUBAO_IMAGE_GENERATION_MODELS = [
  'doubao-seedream-5-0-lite-260128',
  'doubao-seedream-4-5-251128'
] as const

/** Mainstream Doubao video models (Seedance Lite default, 2.0 optional). */
export const DOUBAO_VIDEO_GENERATION_MODELS = [
  'doubao-seedance-1-0-lite-t2v-250428',
  'doubao-seedance-1-0-lite-i2v-250428',
  'doubao-seedance-2-0-260128',
  'doubao-seedance-2-0-fast-260128'
] as const

export const DOUBAO_GENERATION_MODELS = [
  ...DOUBAO_IMAGE_GENERATION_MODELS,
  ...DOUBAO_VIDEO_GENERATION_MODELS
] as const
