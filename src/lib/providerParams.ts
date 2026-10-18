import type { ModelRuntimeOverrides, ProviderConfig } from '../types/chat'
import {
  defaultProtocolForTemplate,
  parseThinkingIntensity,
  parseThinkingProtocol,
  type ThinkingIntensity,
  type ThinkingProtocol
} from './thinkingIntensity'

/** Default Qwen `thinking_budget` when deep thinking is enabled. */
export const DEFAULT_THINKING_BUDGET = 2048

export type ReasoningEffort = 'high' | 'max'

/** UI / preset kind; drives the same RuntimeParamsForm variant as built-in Qwen & DeepSeek. */
export type ProviderTemplateId =
  | 'qwen'
  | 'deepseek'
  | 'doubao'
  | 'openai_compatible'
  | 'openrouter'
  | 'kimi'
  | 'zhipu'
  | 'mimo'

export interface ProviderTemplateMeta {
  id: ProviderTemplateId
  label: string
  /** Shown under the type picker in settings. */
  hint: string
  defaultId: string
  defaultName: string
  defaultBaseUrl: string
  defaultModels: string[]
}

export const PROVIDER_TEMPLATE_OPTIONS: ProviderTemplateMeta[] = [
  {
    id: 'deepseek',
    label: 'DeepSeek',
    hint: '深度求索',
    defaultId: 'deepseek',
    defaultName: 'DeepSeek',
    defaultBaseUrl: 'https://api.deepseek.com/v1',
    defaultModels: ['deepseek-flash', 'deepseek-v4-pro']
  },
  {
    id: 'qwen',
    label: 'Qwen',
    hint: '通义千问',
    defaultId: 'qwen',
    defaultName: 'Qwen',
    defaultBaseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    defaultModels: ['qwen3.8-max', 'qwen3.8-omni-flash', 'qwen3.8-flash', 'qwen3.8-27b']
  },
  {
    id: 'mimo',
    label: 'MiMo',
    hint: '小米 MiMo',
    defaultId: 'mimo',
    defaultName: 'MiMo',
    defaultBaseUrl: 'https://api.xiaomimimo.com/v1',
    defaultModels: ['mimo-v2.6-pro', 'mimo-v2.6-flash', 'mimo-v2.6-pro-ultraspeed']
  },
  {
    id: 'kimi',
    label: 'Kimi',
    hint: '月之暗面',
    defaultId: 'kimi',
    defaultName: 'Kimi',
    defaultBaseUrl: 'https://api.moonshot.cn/v1',
    defaultModels: ['kimi-k3', 'kimi-k2.7-code', 'kimi-k2.7-code-highspeed', 'kimi-k2.6']
  },
  {
    id: 'zhipu',
    label: 'GLM',
    hint: '智谱 GLM',
    defaultId: 'zhipu',
    defaultName: 'GLM',
    defaultBaseUrl: 'https://open.bigmodel.cn/api/paas/v4',
    defaultModels: ['glm-5.3', 'glm-5.3-flash', 'glm-5.3-flashx', 'glm-5.2']
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    hint: '聚合入口',
    defaultId: 'openrouter',
    defaultName: 'OpenRouter',
    defaultBaseUrl: 'https://openrouter.ai/api/v1',
    defaultModels: [
      'stepfun/step-3.7-flash',
      'openai/gpt-5.4-nano',
      'openai/gpt-5.4-mini',
      'openai/gpt-5.6-luna',
      'openai/gpt-5.6-luna-pro',
      'openai/gpt-5.6-terra',
      'openai/gpt-5.6-sol',
      'anthropic/claude-opus-5',
      'anthropic/claude-fable-5',
      'google/gemini-3.6-flash'
    ]
  },
  {
    id: 'openai_compatible',
    label: 'Other',
    hint: '自定义 OpenAI 兼容地址',
    defaultId: '',
    defaultName: '',
    defaultBaseUrl: 'https://api.openai.com/v1',
    defaultModels: []
  }
]

/** First preset whose default id is free. Falls back to a blank custom endpoint. */
export function firstPresetTemplate(occupiedIds: ReadonlySet<string>): ProviderTemplateId {
  const preset = PROVIDER_TEMPLATE_OPTIONS.find(
    option => option.defaultId && !occupiedIds.has(option.defaultId)
  )
  return preset?.id ?? 'openai_compatible'
}

function providerIdLower(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): string {
  return typeof p?.id === 'string' ? p.id.toLowerCase() : ''
}

export function isQwenProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'qwen') return true
  return /dashscope\.aliyuncs\.com|dashscope-intl\.aliyuncs\.com/i.test(p.baseUrl || '')
}

export function isDeepSeekProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'deepseek') return true
  return /api\.deepseek\.com/i.test(p.baseUrl || '')
}

/** Default web-search tier when a DeepSeek provider is configured. The first model is the catalog default. */
export function preferredWebSearchTier(
  providers: Array<Pick<ProviderConfig, 'id' | 'baseUrl' | 'models'>>
): { providerId: string; model: string } | undefined {
  const provider = providers.find(p => isDeepSeekProvider(p))
  const model = provider?.models?.map(item => item.trim()).find(item => item.length > 0)
  if (!provider || !model) return undefined
  return { providerId: provider.id, model }
}

/** Qwen uses DashScope search. Any configured DeepSeek model can select the Search API route. */
export function modelSupportsHostedWebSearch(
  provider: Pick<ProviderConfig, 'id' | 'baseUrl'>,
  model: string
): boolean {
  if (isQwenProvider(provider)) return true
  if (!isDeepSeekProvider(provider)) return false
  return model.trim().length > 0
}

export function isDoubaoProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'doubao') return true
  return /ark\.cn-[a-z-]+\.volces\.com/i.test(p.baseUrl || '')
}

export function isOpenRouterProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'openrouter') return true
  return /openrouter\.ai/i.test(p.baseUrl || '')
}

export function isKimiProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'kimi') return true
  return /moonshot\.(cn|ai)|api\.kimi\.com/i.test(p.baseUrl || '')
}

export function isZhipuProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  const id = providerIdLower(p)
  if (id === 'zhipu' || id === 'glm') return true
  return /bigmodel\.cn/i.test(p.baseUrl || '')
}

export function isMimoProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'mimo') return true
  return /xiaomimimo\.com/i.test(p.baseUrl || '')
}

export function detectProviderTemplateId(
  p: Pick<ProviderConfig, 'id' | 'baseUrl'>
): ProviderTemplateId {
  if (isQwenProvider(p)) return 'qwen'
  if (isDeepSeekProvider(p)) return 'deepseek'
  if (isDoubaoProvider(p)) return 'doubao'
  if (isOpenRouterProvider(p)) return 'openrouter'
  if (isKimiProvider(p)) return 'kimi'
  if (isZhipuProvider(p)) return 'zhipu'
  if (isMimoProvider(p)) return 'mimo'
  return 'openai_compatible'
}

export function providerTemplateMeta(id: ProviderTemplateId): ProviderTemplateMeta {
  return (
    PROVIDER_TEMPLATE_OPTIONS.find(t => t.id === id)
    ?? PROVIDER_TEMPLATE_OPTIONS.find(t => t.id === 'openai_compatible')
    ?? PROVIDER_TEMPLATE_OPTIONS[0]
  )
}

/** Remove provider/model fields that do not apply to the detected template. */
export function stripProviderExtensionFields(
  p: ProviderConfig,
  template: ProviderTemplateId
): ProviderConfig {
  const out: ProviderConfig = { ...p }
  // thinkingIntensity / thinkingProtocol apply to all strategies.
  if (template === 'doubao') {
    delete out.enableThinking
    delete out.thinkingBudget
    delete out.reasoningEffort
  } else if (template === 'qwen') {
    if (out.enableThinking !== true) delete out.thinkingBudget
  }
  if (!out.thinkingProtocol) {
    out.thinkingProtocol = defaultProtocolForTemplate(template)
  }
  const mc = out.modelConfigs
  if (!mc) return out
  const nextMc: Record<string, ModelRuntimeOverrides> = {}
  for (const [mid, raw] of Object.entries(mc)) {
    const o: ModelRuntimeOverrides = { ...raw }
    if (template === 'doubao') {
      delete o.enableThinking
      delete o.thinkingBudget
      delete o.reasoningEffort
    } else if (template === 'qwen' && o.enableThinking !== true) {
      delete o.thinkingBudget
    }
    if (Object.keys(o).length) nextMc[mid] = o
  }
  out.modelConfigs = nextMc
  return out
}

/** Apply template defaults when adding a provider (same flow as built-in Qwen / DeepSeek). */
export function providerDraftForTemplate(
  template: ProviderTemplateId,
  global: { temperature: number; maxTokens: number; contextBudgetTokens?: number }
): ProviderConfig {
  const meta = providerTemplateMeta(template)
  return {
    id: meta.defaultId,
    name: meta.defaultName,
    baseUrl: meta.defaultBaseUrl,
    apiKey: '',
    models: [...meta.defaultModels],
    reasoningInMessages: false,
    temperature: global.temperature,
    maxTokens: global.maxTokens,
    contextBudgetTokens: global.contextBudgetTokens,
    modelConfigs: {},
    thinkingProtocol: defaultProtocolForTemplate(template)
  }
}

export function normalizeReasoningEffort(v: unknown): ReasoningEffort | undefined {
  if (v === 'high' || v === 'max') return v
  return undefined
}

export function normalizeThinkingIntensity(v: unknown): ThinkingIntensity | undefined {
  const parsed = parseThinkingIntensity(v)
  return parsed || undefined
}

export function normalizeThinkingProtocol(v: unknown): ThinkingProtocol | undefined {
  const parsed = parseThinkingProtocol(v)
  return parsed || undefined
}
