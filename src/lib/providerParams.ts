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
export type ProviderTemplateId = 'qwen' | 'deepseek' | 'doubao' | 'openai_compatible' | 'openrouter' | 'kimi' | 'zhipu'

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
    id: 'openai_compatible',
    label: 'OpenAI 兼容',
    hint: '其它 OpenAI 格式端点；可配 extra_body（如 repetition_penalty）',
    defaultId: '',
    defaultName: '',
    defaultBaseUrl: 'https://api.openai.com/v1',
    defaultModels: []
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    hint: '聚合入口；可调用 GPT-5.6 / Claude / Gemini 等',
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
    id: 'kimi',
    label: 'Kimi',
    hint: '月之暗面 API；长文档处理突出',
    defaultId: 'kimi',
    defaultName: 'Kimi（月之暗面）',
    defaultBaseUrl: 'https://api.moonshot.cn/v1',
    defaultModels: ['kimi-k3']
  },
  {
    id: 'zhipu',
    label: '智谱 GLM',
    hint: '智谱 API；代码生成突出',
    defaultId: 'zhipu',
    defaultName: '智谱 GLM',
    defaultBaseUrl: 'https://open.bigmodel.cn/api/paas/v4',
    defaultModels: ['glm-5.2']
  }
]

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
  return /moonshot\.cn/i.test(p.baseUrl || '')
}

export function isZhipuProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'zhipu') return true
  return /bigmodel\.cn/i.test(p.baseUrl || '')
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
