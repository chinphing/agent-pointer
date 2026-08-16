import type { ModelRuntimeOverrides, ProviderConfig } from '../types/chat'

export interface ModelCapabilityFlags {
  supportsVision?: boolean
  canGenerateImage?: boolean
  canGenerateVideo?: boolean
}

/** Fixed provider defaults for vision (no model-name heuristics). */
export function providerDefaultSupportsVision(providerId: string): boolean | undefined {
  const id = providerId.trim().toLowerCase()
  if (id === 'qwen') return true
  if (id === 'deepseek') return false
  return undefined
}

/** Generation capability hints from model id (image/video tools only). */
export function inferModelGenerationCapabilities(modelId: string): ModelCapabilityFlags {
  const m = modelId.trim().toLowerCase()
  if (!m) return {}

  const flags: ModelCapabilityFlags = {}

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

/** Speech-to-text / ASR models (aligned with Rust `dashscope_audio_model_id`). */
export function inferModelAudioTranscription(modelId: string): boolean {
  const m = modelId.trim().toLowerCase()
  if (!m) return false
  return (
    m.includes('asr') ||
    m.includes('whisper') ||
    (m.includes('audio') && !m.includes('seedance'))
  )
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
  const inferred = inferModelGenerationCapabilities(model)
  const providerVision = providerDefaultSupportsVision(providerId)
  return {
    supportsVision: over?.supportsVision ?? providerVision ?? false,
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

export function modelSupportsAudioTranscription(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): boolean {
  void providers
  void providerId
  return inferModelAudioTranscription(model)
}

/** Seed provider-fixed vision defaults and generation flags (no overwrite of explicit values). */
export function seedProviderModelCapabilities(provider: ProviderConfig): ProviderConfig {
  const configs = { ...(provider.modelConfigs ?? {}) }
  const providerVision = providerDefaultSupportsVision(provider.id)
  for (const model of provider.models ?? []) {
    const inferred = inferModelGenerationCapabilities(model)
    const prev = configs[model] ?? {}
    const next: ModelRuntimeOverrides = { ...prev }
    if (provider.id === 'deepseek') {
      next.supportsVision = false
    } else if (provider.id === 'qwen') {
      if (next.supportsVision === undefined) next.supportsVision = true
    } else if (next.supportsVision === undefined && providerVision !== undefined) {
      next.supportsVision = providerVision
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
