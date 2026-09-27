import type { Ref } from 'vue'
import { t } from '../i18n'
import type { ModelRuntimeOverrides, ProviderConfig } from '../types/chat'
import {
  DEFAULT_THINKING_BUDGET,
  detectProviderTemplateId,
  isDeepSeekProvider,
  isQwenProvider,
  normalizeReasoningEffort,
  normalizeThinkingIntensity,
  normalizeThinkingProtocol,
  type ReasoningEffort
} from '../lib/providerParams'
import {
  defaultProtocolForTemplate,
  intensityFromLegacy,
  patchFromThinkingIntensity,
  type ThinkingIntensity,
  type ThinkingProtocol
} from '../lib/thinkingIntensity'

export const DEFAULT_MODEL_TEMPERATURE = 0.7
export const DEFAULT_MODEL_TOP_P = 0.95
export const DEFAULT_MODEL_MAX_TOKENS = 2048
export const DEFAULT_CONTEXT_BUDGET_TOKENS = 262_144
export const CONTEXT_BUDGET_TOKENS_MIN = 4096
export const CONTEXT_BUDGET_TOKENS_MAX = 2_097_152
/** Previous product defaults (decimal 100k/120k and 100/120 Ki). */
const LEGACY_CONTEXT_BUDGET_TOKENS = new Set([100_000, 102_400, 120_000, 122_880])

export type RuntimeGenFallback = {
  temperature: () => number
  maxTokens: () => number
  contextBudgetTokens: () => number
}

export type RuntimeParamsVariant = 'qwen' | 'deepseek' | 'generic'

export function providerParamsVariant(p: ProviderConfig): RuntimeParamsVariant {
  if (isQwenProvider(p)) return 'qwen'
  if (isDeepSeekProvider(p)) return 'deepseek'
  return 'generic'
}

export interface RuntimeParamsApi {
  variant: RuntimeParamsVariant
  temperature: () => number
  setTemperature: (value: number) => void
  topP: () => number
  setTopP: (value: number) => void
  maxTokens: () => number
  setMaxTokens: (value: number) => void
  contextBudgetTokens: () => number
  setContextBudgetTokens: (value: number) => void
  reasoningOn: () => boolean
  setReasoningOn: (on: boolean) => void
  deepThinkingOn: () => boolean
  setDeepThinkingOn: (on: boolean) => void
  thinkingBudget: () => number
  setThinkingBudget: (value: number) => void
  reasoningEffort: () => '' | ReasoningEffort
  setReasoningEffort: (value: string) => void
  thinkingIntensity: () => '' | ThinkingIntensity
  setThinkingIntensity: (value: string) => void
  thinkingProtocol: () => ThinkingProtocol
  setThinkingProtocol: (value: string) => void
  showThinkingIntensity: () => boolean
  showProtocolPicker: () => boolean
  showQwenBudgetControls: () => boolean
  /** Pretty JSON for Hermes-style `extraBody` (empty string = unset). */
  extraBodyJson: () => string
  /** Parse JSON object into `extraBody`; empty clears. */
  setExtraBodyJson: (text: string) => { ok: true } | { ok: false; error: string }
}

/** Normalize a JSON object for `extraBody`; empty / non-object → undefined. */
export function normalizeExtraBody(v: unknown): Record<string, unknown> | undefined {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return undefined
  const keys = Object.keys(v as object)
  if (keys.length === 0) return undefined
  return { ...(v as Record<string, unknown>) }
}

export function formatExtraBodyJson(v: unknown): string {
  const obj = normalizeExtraBody(v)
  if (!obj) return ''
  try {
    return JSON.stringify(obj, null, 2)
  } catch {
    return ''
  }
}

export function parseExtraBodyJson(
  text: string
): { ok: true; value?: Record<string, unknown> } | { ok: false; error: string } {
  const t = text.trim()
  if (!t) return { ok: true, value: undefined }
  try {
    const parsed = JSON.parse(t) as unknown
    const obj = normalizeExtraBody(parsed)
    if (!obj) {
      return { ok: false, error: t('settings.runtime.jsonObjectRequired') }
    }
    return { ok: true, value: obj }
  } catch {
    return { ok: false, error: t('settings.runtime.jsonInvalid') }
  }
}

export function clampContextBudgetTokens(value: number): number {
  const n = Math.round(Number(value))
  if (!Number.isFinite(n)) return DEFAULT_CONTEXT_BUDGET_TOKENS
  const clamped = Math.min(CONTEXT_BUDGET_TOKENS_MAX, Math.max(CONTEXT_BUDGET_TOKENS_MIN, n))
  if (LEGACY_CONTEXT_BUDGET_TOKENS.has(clamped)) return DEFAULT_CONTEXT_BUDGET_TOKENS
  return clamped
}

export function useRuntimeParams(
  provider: Ref<ProviderConfig | null>,
  modelId: Ref<string | null>,
  globalFallback: RuntimeGenFallback
): RuntimeParamsApi {
  function fallbackTemperature(): number {
    const t = globalFallback.temperature()
    return Number.isFinite(t) && t >= 0 ? t : DEFAULT_MODEL_TEMPERATURE
  }

  function fallbackMaxTokens(): number {
    const n = globalFallback.maxTokens()
    return n && n >= 64 ? n : DEFAULT_MODEL_MAX_TOKENS
  }

  function fallbackContextBudgetTokens(): number {
    return clampContextBudgetTokens(
      globalFallback.contextBudgetTokens() || DEFAULT_CONTEXT_BUDGET_TOKENS
    )
  }

  function patchModel(
    id: string,
    patch: (prev: ModelRuntimeOverrides) => ModelRuntimeOverrides
  ) {
    const p = provider.value
    if (!p) return
    const configs = p.modelConfigs ?? {}
    const prev: ModelRuntimeOverrides = {
      temperature: p.temperature ?? fallbackTemperature(),
      topP: p.topP ?? DEFAULT_MODEL_TOP_P,
      maxTokens: p.maxTokens ?? fallbackMaxTokens(),
      contextBudgetTokens: p.contextBudgetTokens ?? fallbackContextBudgetTokens(),
      reasoningInMessages: p.reasoningInMessages === true,
      ...configs[id]
    }
    // 替换 modelConfigs 顶层引用，勿就地改 configs[id]，否则 Pinia/Vue 可能不刷新 UI。
    p.modelConfigs = { ...configs, [id]: patch(prev) }
  }

  function variant(): RuntimeParamsVariant {
    const p = provider.value
    return p ? providerParamsVariant(p) : 'generic'
  }

  function temperature(): number {
    const p = provider.value
    if (!p) return fallbackTemperature()
    const mid = modelId.value
    if (mid) {
      return (
        p.modelConfigs?.[mid]?.temperature
        ?? p.temperature
        ?? fallbackTemperature()
      )
    }
    return p.temperature ?? fallbackTemperature()
  }

  function setTemperature(value: number) {
    const p = provider.value
    if (!p) return
    const v = Math.min(2, Math.max(0, Number(value)))
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => ({ ...prev, temperature: v }))
    } else {
      p.temperature = v
    }
  }

  function topP(): number {
    const p = provider.value
    if (!p) return DEFAULT_MODEL_TOP_P
    const mid = modelId.value
    if (mid) {
      return clampTopP(p.modelConfigs?.[mid]?.topP ?? p.topP ?? DEFAULT_MODEL_TOP_P)
    }
    return clampTopP(p.topP ?? DEFAULT_MODEL_TOP_P)
  }

  function setTopP(value: number) {
    const p = provider.value
    if (!p) return
    const v = clampTopP(value)
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => ({ ...prev, topP: v }))
    } else {
      p.topP = v
    }
  }

  function maxTokens(): number {
    const p = provider.value
    if (!p) return fallbackMaxTokens()
    const mid = modelId.value
    if (mid) {
      return (
        p.modelConfigs?.[mid]?.maxTokens
        ?? p.maxTokens
        ?? fallbackMaxTokens()
      )
    }
    return p.maxTokens ?? fallbackMaxTokens()
  }

  function setMaxTokens(value: number) {
    const p = provider.value
    if (!p) return
    const v = Math.min(64000, Math.max(64, Math.round(Number(value))))
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => ({ ...prev, maxTokens: v }))
    } else {
      p.maxTokens = v
    }
  }

  function contextBudgetTokens(): number {
    const p = provider.value
    if (!p) return fallbackContextBudgetTokens()
    const mid = modelId.value
    if (mid) {
      return clampContextBudgetTokens(
        p.modelConfigs?.[mid]?.contextBudgetTokens
        ?? p.contextBudgetTokens
        ?? fallbackContextBudgetTokens()
      )
    }
    return clampContextBudgetTokens(p.contextBudgetTokens ?? fallbackContextBudgetTokens())
  }

  function setContextBudgetTokens(value: number) {
    const p = provider.value
    if (!p) return
    const v = clampContextBudgetTokens(value)
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => ({ ...prev, contextBudgetTokens: v }))
    } else {
      p.contextBudgetTokens = v
    }
  }

  function reasoningOn(): boolean {
    const p = provider.value
    if (!p) return true
    const mid = modelId.value
    if (mid) {
      const o = p.modelConfigs?.[mid]?.reasoningInMessages
      if (o !== undefined) return o
    }
    return p.reasoningInMessages === true
  }

  function setReasoningOn(on: boolean) {
    const p = provider.value
    if (!p) return
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => ({ ...prev, reasoningInMessages: on }))
    } else {
      p.reasoningInMessages = on
    }
  }

  function deepThinkingOn(): boolean {
    const p = provider.value
    if (!p) return false
    const mid = modelId.value
    if (mid) {
      return p.modelConfigs?.[mid]?.enableThinking === true
    }
    return p.enableThinking === true
  }

  function setDeepThinkingOn(on: boolean) {
    const p = provider.value
    if (!p) return
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => {
        const next = { ...prev, enableThinking: on }
        if (on) {
          if (!next.thinkingBudget || next.thinkingBudget < 1) {
            next.thinkingBudget = DEFAULT_THINKING_BUDGET
          }
        } else {
          delete next.thinkingBudget
        }
        return next
      })
    } else {
      p.enableThinking = on
      if (on) {
        if (!p.thinkingBudget || p.thinkingBudget < 1) {
          p.thinkingBudget = DEFAULT_THINKING_BUDGET
        }
      } else {
        delete p.thinkingBudget
      }
    }
  }

  function thinkingBudget(): number {
    const p = provider.value
    if (!p) return DEFAULT_THINKING_BUDGET
    const mid = modelId.value
    if (mid) {
      return p.modelConfigs?.[mid]?.thinkingBudget ?? DEFAULT_THINKING_BUDGET
    }
    return p.thinkingBudget ?? DEFAULT_THINKING_BUDGET
  }

  function setThinkingBudget(value: number) {
    const p = provider.value
    if (!p) return
    const v = Math.max(1, Math.round(Number(value)))
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => ({ ...prev, thinkingBudget: v }))
    } else {
      p.thinkingBudget = v
    }
  }

  function reasoningEffort(): '' | ReasoningEffort {
    const p = provider.value
    if (!p) return ''
    const mid = modelId.value
    if (mid) {
      return normalizeReasoningEffort(p.modelConfigs?.[mid]?.reasoningEffort) ?? ''
    }
    return normalizeReasoningEffort(p.reasoningEffort) ?? ''
  }

  function setReasoningEffort(value: string) {
    const p = provider.value
    if (!p) return
    const v = normalizeReasoningEffort(value)
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => {
        const next = { ...prev }
        if (v) next.reasoningEffort = v
        else delete next.reasoningEffort
        return next
      })
    } else if (v) {
      p.reasoningEffort = v
    } else {
      delete p.reasoningEffort
    }
  }

  function thinkingProtocol(): ThinkingProtocol {
    const p = provider.value
    if (!p) return defaultProtocolForTemplate(detectProviderTemplateId(p ?? { id: '', baseUrl: '' }))
    const mid = modelId.value
    const raw = mid
      ? p.modelConfigs?.[mid]?.thinkingProtocol ?? p.thinkingProtocol
      : p.thinkingProtocol
    return (
      normalizeThinkingProtocol(raw) ??
      defaultProtocolForTemplate(detectProviderTemplateId(p))
    )
  }

  function setThinkingProtocol(value: string) {
    const p = provider.value
    if (!p) return
    const v = normalizeThinkingProtocol(value) ?? 'auto'
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => ({ ...prev, thinkingProtocol: v }))
    } else {
      p.thinkingProtocol = v
    }
  }

  function thinkingIntensity(): '' | ThinkingIntensity {
    const p = provider.value
    if (!p) return ''
    const mid = modelId.value
    if (mid) {
      const o = p.modelConfigs?.[mid]
      return intensityFromLegacy({
        thinkingIntensity: o?.thinkingIntensity ?? p.thinkingIntensity,
        enableThinking: o?.enableThinking ?? p.enableThinking,
        thinkingBudget: o?.thinkingBudget ?? p.thinkingBudget,
        reasoningEffort: o?.reasoningEffort ?? p.reasoningEffort
      })
    }
    return intensityFromLegacy(p)
  }

  function setThinkingIntensity(value: string) {
    const p = provider.value
    if (!p) return
    const intensity = (normalizeThinkingIntensity(value) ?? '') as '' | ThinkingIntensity
    const patch = patchFromThinkingIntensity(intensity)
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => {
        const next = { ...prev }
        if (patch.thinkingIntensity) next.thinkingIntensity = patch.thinkingIntensity as ThinkingIntensity
        else delete next.thinkingIntensity
        if (patch.enableThinking !== undefined) next.enableThinking = patch.enableThinking
        else delete next.enableThinking
        if (patch.thinkingBudget !== undefined) next.thinkingBudget = patch.thinkingBudget
        else delete next.thinkingBudget
        if (patch.reasoningEffort) next.reasoningEffort = patch.reasoningEffort
        else delete next.reasoningEffort
        return next
      })
    } else {
      if (patch.thinkingIntensity) p.thinkingIntensity = patch.thinkingIntensity as ThinkingIntensity
      else delete p.thinkingIntensity
      if (patch.enableThinking !== undefined) p.enableThinking = patch.enableThinking
      else delete p.enableThinking
      if (patch.thinkingBudget !== undefined) p.thinkingBudget = patch.thinkingBudget
      else delete p.thinkingBudget
      if (patch.reasoningEffort) p.reasoningEffort = patch.reasoningEffort
      else delete p.reasoningEffort
    }
  }

  function showThinkingIntensity(): boolean {
    return true
  }

  function showProtocolPicker(): boolean {
    return false
  }

  function showQwenBudgetControls(): boolean {
    return false
  }

  function extraBodyJson(): string {
    const p = provider.value
    if (!p) return ''
    const mid = modelId.value
    if (mid) {
      return formatExtraBodyJson(p.modelConfigs?.[mid]?.extraBody)
    }
    return formatExtraBodyJson(p.extraBody)
  }

  function setExtraBodyJson(text: string): { ok: true } | { ok: false; error: string } {
    const p = provider.value
    if (!p) return { ok: false, error: t('settings.runtime.noProvider') }
    const parsed = parseExtraBodyJson(text)
    if (!parsed.ok) return parsed
    const mid = modelId.value
    if (mid) {
      patchModel(mid, prev => {
        const next = { ...prev }
        if (parsed.value) next.extraBody = parsed.value
        else delete next.extraBody
        return next
      })
    } else if (parsed.value) {
      p.extraBody = parsed.value
    } else {
      delete p.extraBody
    }
    return { ok: true }
  }

  return {
    get variant() {
      return variant()
    },
    temperature,
    setTemperature,
    topP,
    setTopP,
    maxTokens,
    setMaxTokens,
    contextBudgetTokens,
    setContextBudgetTokens,
    reasoningOn,
    setReasoningOn,
    deepThinkingOn,
    setDeepThinkingOn,
    thinkingBudget,
    setThinkingBudget,
    reasoningEffort,
    setReasoningEffort,
    thinkingIntensity,
    setThinkingIntensity,
    thinkingProtocol,
    setThinkingProtocol,
    showThinkingIntensity,
    showProtocolPicker,
    showQwenBudgetControls,
    extraBodyJson,
    setExtraBodyJson
  }
}

export function providerDefaultTemperature(
  p: ProviderConfig,
  globalFallback: { temperature: () => number }
): number {
  const t = p.temperature
  if (t !== undefined && Number.isFinite(t) && t >= 0) return t
  const g = globalFallback.temperature()
  return Number.isFinite(g) && g >= 0 ? g : DEFAULT_MODEL_TEMPERATURE
}

export function providerDefaultTopP(p: ProviderConfig): number {
  const v = p.topP
  if (v !== undefined && Number.isFinite(v) && v >= 0 && v <= 1) return v
  return DEFAULT_MODEL_TOP_P
}

export function clampTopP(value: number): number {
  const n = Number(value)
  if (!Number.isFinite(n)) return DEFAULT_MODEL_TOP_P
  return Math.min(1, Math.max(0, n))
}

export function providerDefaultMaxTokens(
  p: ProviderConfig,
  globalFallback: { maxTokens: () => number }
): number {
  const n = p.maxTokens
  if (n !== undefined && n >= 64) return n
  const g = globalFallback.maxTokens()
  return g && g >= 64 ? g : DEFAULT_MODEL_MAX_TOKENS
}

export function providerDefaultContextBudgetTokens(
  p: ProviderConfig,
  globalFallback: { contextBudgetTokens: () => number }
): number {
  if (p.contextBudgetTokens !== undefined) {
    return clampContextBudgetTokens(p.contextBudgetTokens)
  }
  return clampContextBudgetTokens(
    globalFallback.contextBudgetTokens() || DEFAULT_CONTEXT_BUDGET_TOKENS
  )
}

/** True when overrides differ from provider defaults (empty / omitted means「同上」). */
export function hasEffectiveModelOverride(
  o: ModelRuntimeOverrides,
  p: ProviderConfig,
  globalFallback: RuntimeGenFallback,
  modelId = ''
): boolean {
  if (o.thinkingIntensity !== undefined || o.thinkingProtocol !== undefined) {
    return true
  }
  if (o.enableThinking !== undefined || o.thinkingBudget !== undefined) {
    return true
  }
  if (o.reasoningEffort !== undefined) {
    return true
  }
  const providerReasoning = p.reasoningInMessages !== false
  if (o.reasoningInMessages !== undefined && o.reasoningInMessages !== providerReasoning) {
    return true
  }
  if (
    o.temperature !== undefined
    && Math.abs(o.temperature - providerDefaultTemperature(p, globalFallback)) > 1e-6
  ) {
    return true
  }
  if (
    o.maxTokens !== undefined
    && o.maxTokens !== providerDefaultMaxTokens(p, globalFallback)
  ) {
    return true
  }
  if (
    o.contextBudgetTokens !== undefined
    && clampContextBudgetTokens(o.contextBudgetTokens)
      !== providerDefaultContextBudgetTokens(p, globalFallback)
  ) {
    return true
  }
  const defaultTopP = providerDefaultTopP(p)
  if (o.topP !== undefined && Math.abs(o.topP - defaultTopP) > 1e-6) {
    return true
  }
  // Keep explicit capability flags from catalog / user checkboxes.
  if (o.supportsVision !== undefined) {
    return true
  }
  if (o.supportsAudio !== undefined) {
    return true
  }
  if (o.canGenerateImage !== undefined) {
    return true
  }
  if (o.canGenerateVideo !== undefined) {
    return true
  }
  if (normalizeExtraBody(o.extraBody)) {
    return true
  }
  return false
}

export function pruneInheritedModelConfigs(
  p: ProviderConfig,
  configs: ProviderConfig['modelConfigs'] | undefined,
  globalFallback: RuntimeGenFallback
): NonNullable<ProviderConfig['modelConfigs']> {
  const src = configs ?? {}
  const out: Record<string, ModelRuntimeOverrides> = {}
  for (const [k, v] of Object.entries(src)) {
    if (hasEffectiveModelOverride(v, p, globalFallback, k)) {
      out[k] = { ...v }
    }
  }
  return out
}

/** Keep only explicit, effective per-model overrides for provider save snapshots. */
export function sanitizeProviderModelConfigs(
  provider: ProviderConfig,
  modelIds: string[],
  configs: ProviderConfig['modelConfigs'] | undefined,
  globalFallback: RuntimeGenFallback
): NonNullable<ProviderConfig['modelConfigs']> {
  const nextMc: Record<string, ModelRuntimeOverrides> = {}
  const src = configs ?? {}
  for (const id of modelIds) {
    const o = src[id]
    if (!o) continue
    const clean: ModelRuntimeOverrides = {}
    if (o.reasoningInMessages !== undefined) clean.reasoningInMessages = o.reasoningInMessages
    if (o.temperature !== undefined) clean.temperature = o.temperature
    if (o.topP !== undefined) clean.topP = o.topP
    if (o.maxTokens !== undefined) clean.maxTokens = o.maxTokens
    if (o.contextBudgetTokens !== undefined) {
      clean.contextBudgetTokens = clampContextBudgetTokens(o.contextBudgetTokens)
    }
    if (o.thinkingIntensity !== undefined) clean.thinkingIntensity = o.thinkingIntensity
    if (o.thinkingProtocol !== undefined) clean.thinkingProtocol = o.thinkingProtocol
    if (o.reasoningEffort !== undefined) clean.reasoningEffort = o.reasoningEffort
    if (o.enableThinking !== undefined) clean.enableThinking = o.enableThinking
    if (o.enableThinking === true && o.thinkingBudget !== undefined) {
      clean.thinkingBudget = o.thinkingBudget
    }
    if (o.supportsVision !== undefined) clean.supportsVision = o.supportsVision
    if (o.supportsAudio !== undefined) clean.supportsAudio = o.supportsAudio
    if (o.canGenerateImage !== undefined) clean.canGenerateImage = o.canGenerateImage
    if (o.canGenerateVideo !== undefined) clean.canGenerateVideo = o.canGenerateVideo
    const extra = normalizeExtraBody(o.extraBody)
    if (extra) clean.extraBody = extra
    if (
      Object.keys(clean).length
      && hasEffectiveModelOverride(clean, provider, globalFallback, id)
    ) {
      nextMc[id] = clean
    }
  }
  return nextMc
}

/** Patch one capability flag on a provider draft (replaces modelConfigs reference). */
export function patchProviderModelCapability(
  provider: ProviderConfig,
  modelId: string,
  flag: 'supportsVision' | 'supportsAudio' | 'canGenerateImage' | 'canGenerateVideo',
  value: boolean
): ProviderConfig {
  const configs = { ...(provider.modelConfigs ?? {}) }
  configs[modelId] = { ...(configs[modelId] ?? {}), [flag]: value }
  return { ...provider, modelConfigs: configs }
}

/** Build per-model custom entry from provider defaults (explicit fields). */
export function buildCustomModelEntryFromProvider(
  p: ProviderConfig,
  globalFallback: RuntimeGenFallback
): ModelRuntimeOverrides {
  const temp = globalFallback.temperature()
  const max = globalFallback.maxTokens()
  const ctx = globalFallback.contextBudgetTokens()
  const entry: ModelRuntimeOverrides = {
    temperature: p.temperature ?? (Number.isFinite(temp) && temp >= 0 ? temp : DEFAULT_MODEL_TEMPERATURE),
    maxTokens: p.maxTokens ?? (max && max >= 64 ? max : DEFAULT_MODEL_MAX_TOKENS),
    contextBudgetTokens: clampContextBudgetTokens(
      p.contextBudgetTokens ?? ctx ?? DEFAULT_CONTEXT_BUDGET_TOKENS
    ),
    reasoningInMessages: p.reasoningInMessages === true
  }
  if (isQwenProvider(p)) {
    entry.enableThinking = p.enableThinking === true
    if (entry.enableThinking) {
      entry.thinkingBudget = p.thinkingBudget ?? DEFAULT_THINKING_BUDGET
    }
  }
  if (p.thinkingIntensity) entry.thinkingIntensity = p.thinkingIntensity
  if (p.thinkingProtocol) entry.thinkingProtocol = p.thinkingProtocol
  if (p.reasoningEffort) {
    entry.reasoningEffort = normalizeReasoningEffort(p.reasoningEffort) ?? p.reasoningEffort
  }
  return entry
}
