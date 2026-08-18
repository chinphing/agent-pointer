import type { ComputerTierLlmConfig } from '../types/chat'

/** Unified product thinking intensity ladder. */
export type ThinkingIntensity = 'off' | 'low' | 'medium' | 'high' | 'max'

/** Wire strategy id stored as `thinkingProtocol`. */
export type ThinkingProtocol =
  | 'auto'
  | 'budget'
  | 'effort'
  | 'openrouter'
  | 'kimi'
  | 'openai_effort'
  | 'off'
  | 'custom'

/** Narrow ladder still used by DeepSeek-only scene dropdowns. */
export type EffortThinkingIntensity = 'off' | 'high' | 'max'

export const THINKING_INTENSITY_OPTIONS: { value: '' | ThinkingIntensity; label: string }[] = [
  { value: '', label: '不设置' },
  { value: 'off', label: '关闭' },
  { value: 'low', label: '低' },
  { value: 'medium', label: '中' },
  { value: 'high', label: '高' },
  { value: 'max', label: '最高' }
]

export const THINKING_PROTOCOL_OPTIONS: { value: ThinkingProtocol; label: string }[] = [
  { value: 'auto', label: '自动' },
  { value: 'budget', label: '千问预算' },
  { value: 'effort', label: 'DeepSeek 力度' },
  { value: 'openrouter', label: 'OpenRouter' },
  { value: 'kimi', label: 'Kimi' },
  { value: 'openai_effort', label: 'OpenAI 力度' },
  { value: 'off', label: '关闭结构化' },
  { value: 'custom', label: '仅扩展参数' }
]

export function parseThinkingIntensity(raw: unknown): ThinkingIntensity | '' {
  if (typeof raw !== 'string') return ''
  const v = raw.trim().toLowerCase()
  if (v === 'off' || v === 'none' || v === 'disabled') return 'off'
  if (v === 'low' || v === 'minimal') return 'low'
  if (v === 'medium' || v === 'med' || v === 'standard') return 'medium'
  if (v === 'high') return 'high'
  if (v === 'max' || v === 'xhigh' || v === 'highest') return 'max'
  return ''
}

export function parseThinkingProtocol(raw: unknown): ThinkingProtocol | '' {
  if (typeof raw !== 'string') return ''
  const v = raw.trim().toLowerCase()
  if (
    v === 'auto' ||
    v === 'budget' ||
    v === 'effort' ||
    v === 'openrouter' ||
    v === 'kimi' ||
    v === 'openai_effort' ||
    v === 'off' ||
    v === 'custom'
  ) {
    return v
  }
  if (v === 'qwen') return 'budget'
  if (v === 'deepseek') return 'effort'
  if (v === 'openai' || v === 'zhipu' || v === 'glm') return 'openai_effort'
  return ''
}

/** Infer intensity from legacy enableThinking / thinkingBudget / reasoningEffort. */
export function intensityFromLegacy(config: {
  thinkingIntensity?: string
  enableThinking?: boolean
  thinkingBudget?: number
  reasoningEffort?: string
}): ThinkingIntensity | '' {
  const direct = parseThinkingIntensity(config.thinkingIntensity)
  if (direct) return direct
  if (config.enableThinking === false) return 'off'
  const effort = parseThinkingIntensity(config.reasoningEffort)
  if (effort) return effort
  if (config.reasoningEffort === 'high') return 'high'
  if (config.reasoningEffort === 'max') return 'max'
  const budget = config.thinkingBudget
  if (typeof budget === 'number' && budget > 0) {
    if (budget < 1536) return 'low'
    if (budget < 3072) return 'medium'
    if (budget < 6144) return 'high'
    return 'max'
  }
  if (config.enableThinking === true) return 'medium'
  return ''
}

export function budgetForIntensity(intensity: ThinkingIntensity): number | undefined {
  switch (intensity) {
    case 'off':
      return undefined
    case 'low':
      return 1024
    case 'medium':
      return 2048
    case 'high':
      return 4096
    case 'max':
      return 8192
  }
}

/** Patch fields when user picks an intensity (keeps legacy fields in sync). */
export function patchFromThinkingIntensity(intensity: '' | ThinkingIntensity): {
  thinkingIntensity?: string
  enableThinking?: boolean
  thinkingBudget?: number
  reasoningEffort?: 'high' | 'max'
} {
  if (!intensity) {
    return {
      thinkingIntensity: undefined,
      enableThinking: undefined,
      thinkingBudget: undefined,
      reasoningEffort: undefined
    }
  }
  if (intensity === 'off') {
    return {
      thinkingIntensity: 'off',
      enableThinking: false,
      thinkingBudget: undefined,
      reasoningEffort: undefined
    }
  }
  const budget = budgetForIntensity(intensity)
  const out: {
    thinkingIntensity: string
    enableThinking: boolean
    thinkingBudget?: number
    reasoningEffort?: 'high' | 'max'
  } = {
    thinkingIntensity: intensity,
    enableThinking: true,
    thinkingBudget: budget
  }
  if (intensity === 'high') out.reasoningEffort = 'high'
  if (intensity === 'max') out.reasoningEffort = 'max'
  return out
}

export function defaultProtocolForTemplate(
  template: 'qwen' | 'deepseek' | 'doubao' | 'openai_compatible' | 'openrouter' | 'kimi' | 'zhipu'
): ThinkingProtocol {
  switch (template) {
    case 'qwen':
      return 'budget'
    case 'deepseek':
      return 'effort'
    case 'openrouter':
      return 'openrouter'
    case 'kimi':
      return 'kimi'
    case 'zhipu':
      return 'openai_effort'
    case 'doubao':
      return 'off'
    default:
      return 'auto'
  }
}

export function strategyShowsIntensity(protocol: ThinkingProtocol | ''): boolean {
  const p = protocol || 'auto'
  return p !== 'off' && p !== 'custom'
}

/** Scene-tier / debug UI intensity for effort-protocol providers (DeepSeek). */
export function patchEffortThinking(value: EffortThinkingIntensity): Partial<ComputerTierLlmConfig> {
  if (value === 'off') {
    return {
      thinkingIntensity: 'off',
      enableThinking: false,
      reasoningEffort: undefined,
      thinkingBudget: undefined
    }
  }
  if (value === 'max') {
    return {
      thinkingIntensity: 'max',
      enableThinking: true,
      reasoningEffort: 'max',
      thinkingBudget: undefined
    }
  }
  return {
    thinkingIntensity: 'high',
    enableThinking: true,
    reasoningEffort: 'high',
    thinkingBudget: undefined
  }
}

export function effortThinkingValue(config: {
  thinkingIntensity?: string
  enableThinking?: boolean
  thinkingBudget?: number
  reasoningEffort?: string
}): EffortThinkingIntensity {
  const intensity = intensityFromLegacy(config)
  if (intensity === 'max') return 'max'
  if (intensity === 'high') return 'high'
  if (intensity === 'off') return 'off'
  if (config.reasoningEffort === 'max') return 'max'
  if (config.reasoningEffort === 'high') return 'high'
  if (config.enableThinking === false) return 'off'
  return 'off'
}

/** Full intensity ladder for scene / custom providers. */
export function patchTierThinkingIntensity(
  value: '' | ThinkingIntensity
): Partial<ComputerTierLlmConfig> {
  const patch = patchFromThinkingIntensity(value)
  return {
    thinkingIntensity: patch.thinkingIntensity as ThinkingIntensity | undefined,
    enableThinking: patch.enableThinking,
    thinkingBudget: patch.thinkingBudget,
    reasoningEffort: patch.reasoningEffort
  }
}

/** Copy a provider/model catalog thinking default into a scene-tier mapping. */
export function thinkingPatchFromProviderModel(
  providers: { id: string; thinkingIntensity?: string; enableThinking?: boolean; thinkingBudget?: number; reasoningEffort?: 'high' | 'max'; modelConfigs?: Record<string, {
    thinkingIntensity?: string
    enableThinking?: boolean
    thinkingBudget?: number
    reasoningEffort?: 'high' | 'max'
  }> }[],
  providerId: string,
  model: string
): Partial<ComputerTierLlmConfig> {
  const provider = providers.find(item => item.id === providerId)
  const over = provider?.modelConfigs?.[model]
  return patchTierThinkingIntensity(
    intensityFromLegacy({
      thinkingIntensity: over?.thinkingIntensity ?? provider?.thinkingIntensity,
      enableThinking: over?.enableThinking ?? provider?.enableThinking,
      thinkingBudget: over?.thinkingBudget ?? provider?.thinkingBudget,
      reasoningEffort: over?.reasoningEffort ?? provider?.reasoningEffort
    })
  )
}

const TIER_THINKING_KEYS = [
  'thinkingIntensity',
  'enableThinking',
  'thinkingBudget',
  'reasoningEffort'
] as const

/** Merge a tier patch; `undefined` thinking fields mean 「不设置」 and drop prior values. */
export function mergeTierLlmPatch(
  prev: ComputerTierLlmConfig,
  patch: Partial<ComputerTierLlmConfig>
): ComputerTierLlmConfig {
  const next: ComputerTierLlmConfig = { ...prev, ...patch }
  for (const key of TIER_THINKING_KEYS) {
    if (Object.prototype.hasOwnProperty.call(patch, key) && patch[key] === undefined) {
      delete next[key]
    }
  }
  return next
}

export function tierThinkingIntensityValue(config: {
  thinkingIntensity?: string
  enableThinking?: boolean
  thinkingBudget?: number
  reasoningEffort?: string
}): '' | ThinkingIntensity {
  return intensityFromLegacy(config)
}

/** Scene mapping always uses the unified intensity control. */
export function templateShowsThinkingIntensity(
  _template?: string
): boolean {
  return true
}
