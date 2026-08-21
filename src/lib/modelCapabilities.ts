import type { ModelRuntimeOverrides, ProviderConfig } from '../types/chat'

export interface ModelCapabilityFlags {
  supportsVision?: boolean
  supportsAudio?: boolean
  canGenerateImage?: boolean
  canGenerateVideo?: boolean
}

function modelOverride(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): ModelRuntimeOverrides | undefined {
  const p = providers.find(x => x.id === providerId)
  return p?.modelConfigs?.[model.trim()]
}

/** Capabilities come only from catalog / user checkboxes. Unset is off. */
export function resolvedModelCapabilities(
  providers: ProviderConfig[],
  providerId: string,
  model: string
): Required<ModelCapabilityFlags> {
  const over = modelOverride(providers, providerId, model)
  return {
    supportsVision: over?.supportsVision === true,
    supportsAudio: over?.supportsAudio === true,
    canGenerateImage: over?.canGenerateImage === true,
    canGenerateVideo: over?.canGenerateVideo === true
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
  return resolvedModelCapabilities(providers, providerId, model).supportsAudio
}
