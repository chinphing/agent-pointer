import type { Ref } from 'vue'
import type { ModelRuntimeOverrides, ProviderConfig } from '../types/chat'
import {
  DEFAULT_THINKING_BUDGET,
  isDeepSeekProvider,
  isQwenProvider,
  normalizeReasoningEffort,
  type ReasoningEffort
} from '../lib/providerParams'

export const DEFAULT_MODEL_TEMPERATURE = 0.7
export const DEFAULT_MODEL_MAX_TOKENS = 2048

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
  maxTokens: () => number
  setMaxTokens: (value: number) => void
  reasoningOn: () => boolean
  setReasoningOn: (on: boolean) => void
  deepThinkingOn: () => boolean
  setDeepThinkingOn: (on: boolean) => void
  thinkingBudget: () => number
  setThinkingBudget: (value: number) => void
  reasoningEffort: () => '' | ReasoningEffort
  setReasoningEffort: (value: string) => void
}

export function useRuntimeParams(
  provider: Ref<ProviderConfig | null>,
  modelId: Ref<string | null>,
  globalFallback: { temperature: () => number; maxTokens: () => number }
): RuntimeParamsApi {
  function fallbackTemperature(): number {
    const t = globalFallback.temperature()
    return Number.isFinite(t) && t >= 0 ? t : DEFAULT_MODEL_TEMPERATURE
  }

  function fallbackMaxTokens(): number {
    const n = globalFallback.maxTokens()
    return n && n >= 64 ? n : DEFAULT_MODEL_MAX_TOKENS
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
      maxTokens: p.maxTokens ?? fallbackMaxTokens(),
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

  return {
    get variant() {
      return variant()
    },
    temperature,
    setTemperature,
    maxTokens,
    setMaxTokens,
    reasoningOn,
    setReasoningOn,
    deepThinkingOn,
    setDeepThinkingOn,
    thinkingBudget,
    setThinkingBudget,
    reasoningEffort,
    setReasoningEffort
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

export function providerDefaultMaxTokens(
  p: ProviderConfig,
  globalFallback: { maxTokens: () => number }
): number {
  const n = p.maxTokens
  if (n !== undefined && n >= 64) return n
  const g = globalFallback.maxTokens()
  return g && g >= 64 ? g : DEFAULT_MODEL_MAX_TOKENS
}

/** True when overrides differ from provider defaults (empty / omitted means「同上」). */
export function hasEffectiveModelOverride(
  o: ModelRuntimeOverrides,
  p: ProviderConfig,
  globalFallback: { temperature: () => number; maxTokens: () => number }
): boolean {
  if (isQwenProvider(p) && (o.enableThinking !== undefined || o.thinkingBudget !== undefined)) {
    return true
  }
  if (isDeepSeekProvider(p) && o.reasoningEffort !== undefined) {
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
  return false
}

export function pruneInheritedModelConfigs(
  p: ProviderConfig,
  configs: ProviderConfig['modelConfigs'] | undefined,
  globalFallback: { temperature: () => number; maxTokens: () => number }
): NonNullable<ProviderConfig['modelConfigs']> {
  const src = configs ?? {}
  const out: Record<string, ModelRuntimeOverrides> = {}
  for (const [k, v] of Object.entries(src)) {
    if (hasEffectiveModelOverride(v, p, globalFallback)) {
      out[k] = { ...v }
    }
  }
  return out
}

/** Build per-model custom entry from provider defaults (explicit fields). */
export function buildCustomModelEntryFromProvider(
  p: ProviderConfig,
  globalFallback: { temperature: () => number; maxTokens: () => number }
): ModelRuntimeOverrides {
  const temp = globalFallback.temperature()
  const max = globalFallback.maxTokens()
  const entry: ModelRuntimeOverrides = {
    temperature: p.temperature ?? (Number.isFinite(temp) && temp >= 0 ? temp : DEFAULT_MODEL_TEMPERATURE),
    maxTokens: p.maxTokens ?? (max && max >= 64 ? max : DEFAULT_MODEL_MAX_TOKENS),
    reasoningInMessages: p.reasoningInMessages === true
  }
  if (isQwenProvider(p)) {
    entry.enableThinking = p.enableThinking === true
    if (entry.enableThinking) {
      entry.thinkingBudget = p.thinkingBudget ?? DEFAULT_THINKING_BUDGET
    }
  }
  if (isDeepSeekProvider(p)) {
    // 显式写入，便于定制弹窗展示并与「同上」区分；未设置时默认 high
    entry.reasoningEffort = normalizeReasoningEffort(p.reasoningEffort) ?? 'high'
  }
  return entry
}
