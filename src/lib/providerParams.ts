import providerCatalog from '../../admin/seed/providers.json'
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
  | 'zhipu'
  | 'kimi'
  | 'mimo'
  | 'hunyuan'
  | 'qianfan'
  | 'minimax'
  | 'stepfun'
  | 'openai'
  | 'gemini'
  | 'grok'
  | 'openrouter'
  | 'openai_compatible'

export interface ProviderTemplateMeta {
  id: ProviderTemplateId
  label: string
  /** 按钮上的短名，一行放得下 */
  short: string
  /** Shown under the type picker in settings. */
  hint: string
  defaultId: string
  defaultName: string
  defaultBaseUrl: string
  defaultModels: string[]
}

/**
 * 服务商清单只有一份：`admin/seed/providers.json`。
 * 企业控制面的种子数据和这里的「添加服务」预设读同一个文件，
 * 改一处两边同步，不会出现控制台配好了、客户端却选不到的情况。
 */
interface CatalogProvider {
  slug: string
  name: string
  short?: string
  hint?: string
  seeded?: boolean
  baseUrl: string
  models: Array<{ id: string }>
}

const CATALOG = (providerCatalog as { providers: CatalogProvider[] }).providers

/** 企业已预置的三家排前面，其余按目录顺序。 */
const ORDERED = [...CATALOG].sort(
  (a, b) => Number(Boolean(b.seeded)) - Number(Boolean(a.seeded))
)

export const PROVIDER_TEMPLATE_OPTIONS: ProviderTemplateMeta[] = [
  ...ORDERED.map(provider => ({
    id: provider.slug as ProviderTemplateId,
    label: provider.name,
    short: provider.short ?? provider.name,
    hint: provider.hint ?? provider.name,
    defaultId: provider.slug,
    defaultName: provider.name,
    defaultBaseUrl: provider.baseUrl,
    defaultModels: provider.models.map(m => m.id)
  })),
  {
    id: 'openai_compatible',
    label: 'Other',
    short: '其他',
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

export function isHunyuanProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'hunyuan') return true
  return /hunyuan\.cloud\.tencent\.com|tokenhub.*tencentmaas/i.test(p.baseUrl || '')
}

export function isQianfanProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'qianfan') return true
  return /qianfan\.baidubce\.com/i.test(p.baseUrl || '')
}

export function isMinimaxProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'minimax') return true
  return /api\.minimax\.(cn|io)/i.test(p.baseUrl || '')
}

export function isStepfunProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'stepfun') return true
  return /api\.stepfun\.com/i.test(p.baseUrl || '')
}

export function isOpenaiProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'openai') return true
  return /api\.openai\.com/i.test(p.baseUrl || '')
}

export function isGeminiProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'gemini') return true
  return /generativelanguage\.googleapis\.com/i.test(p.baseUrl || '')
}

export function isGrokProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (providerIdLower(p) === 'grok' || providerIdLower(p) === 'xai') return true
  return /api\.x\.ai/i.test(p.baseUrl || '')
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
  if (isHunyuanProvider(p)) return 'hunyuan'
  if (isQianfanProvider(p)) return 'qianfan'
  if (isMinimaxProvider(p)) return 'minimax'
  if (isStepfunProvider(p)) return 'stepfun'
  if (isOpenaiProvider(p)) return 'openai'
  if (isGeminiProvider(p)) return 'gemini'
  if (isGrokProvider(p)) return 'grok'
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
