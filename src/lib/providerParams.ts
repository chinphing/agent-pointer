import type { ModelRuntimeOverrides, ProviderConfig } from '../types/chat'

/** Default Qwen `thinking_budget` when deep thinking is enabled. */
export const DEFAULT_THINKING_BUDGET = 2048

export type ReasoningEffort = 'high' | 'max'

/** UI / preset kind; drives the same RuntimeParamsForm variant as built-in Qwen & DeepSeek. */
export type ProviderTemplateId = 'qwen' | 'deepseek' | 'openai_compatible'

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
    id: 'qwen',
    label: '千问',
    hint: 'DashScope 兼容；可配深度思考、思考预算',
    defaultId: 'qwen',
    defaultName: '千问',
    defaultBaseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    defaultModels: [
      'qwen3.5-plus',
      'qwen3.5-27b',
      'qwen3.5-flash',
      'qwen3.7-max',
      'qwen3.6-plus',
      'qwen3.6-27b',
      'qwen3.6-flash'
    ]
  },
  {
    id: 'deepseek',
    label: '深度求索',
    hint: 'DeepSeek API；可配推理力度',
    defaultId: 'deepseek',
    defaultName: '深度求索',
    defaultBaseUrl: 'https://api.deepseek.com/v1',
    defaultModels: ['deepseek-v4-flash', 'deepseek-v4-pro']
  },
  {
    id: 'openai_compatible',
    label: 'OpenAI 兼容',
    hint: '其它 OpenAI 格式端点；最大输出、创造性、回传推理',
    defaultId: '',
    defaultName: '',
    defaultBaseUrl: 'https://api.openai.com/v1',
    defaultModels: []
  }
]

export function isQwenProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (p.id?.toLowerCase() === 'qwen') return true
  return /dashscope\.aliyuncs\.com|dashscope-intl\.aliyuncs\.com/i.test(p.baseUrl || '')
}

export function isDeepSeekProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (p.id?.toLowerCase() === 'deepseek') return true
  return /api\.deepseek\.com/i.test(p.baseUrl || '')
}

export function detectProviderTemplateId(
  p: Pick<ProviderConfig, 'id' | 'baseUrl'>
): ProviderTemplateId {
  if (isQwenProvider(p)) return 'qwen'
  if (isDeepSeekProvider(p)) return 'deepseek'
  return 'openai_compatible'
}

export function providerTemplateMeta(id: ProviderTemplateId): ProviderTemplateMeta {
  return PROVIDER_TEMPLATE_OPTIONS.find(t => t.id === id) ?? PROVIDER_TEMPLATE_OPTIONS[2]
}

/** Remove provider/model fields that do not apply to the detected template. */
export function stripProviderExtensionFields(
  p: ProviderConfig,
  template: ProviderTemplateId
): ProviderConfig {
  const out: ProviderConfig = { ...p }
  if (template !== 'qwen') {
    delete out.enableThinking
    delete out.thinkingBudget
  } else if (out.enableThinking !== true) {
    delete out.thinkingBudget
  }
  if (template !== 'deepseek') {
    delete out.reasoningEffort
  }
  const mc = out.modelConfigs
  if (!mc) return out
  const nextMc: Record<string, ModelRuntimeOverrides> = {}
  for (const [mid, raw] of Object.entries(mc)) {
    const o: ModelRuntimeOverrides = { ...raw }
    if (template !== 'qwen') {
      delete o.enableThinking
      delete o.thinkingBudget
    } else if (o.enableThinking !== true) {
      delete o.thinkingBudget
    }
    if (template !== 'deepseek') {
      delete o.reasoningEffort
    }
    if (Object.keys(o).length) nextMc[mid] = o
  }
  out.modelConfigs = nextMc
  return out
}

/** Apply template defaults when adding a provider (same flow as built-in Qwen / DeepSeek). */
export function providerDraftForTemplate(
  template: ProviderTemplateId,
  global: { temperature: number; maxTokens: number }
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
    modelConfigs: {}
  }
}

export function normalizeReasoningEffort(v: unknown): ReasoningEffort | undefined {
  if (v === 'high' || v === 'max') return v
  return undefined
}
