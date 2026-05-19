import type { ProviderConfig } from '../types/chat'

/** Default Qwen `thinking_budget` when deep thinking is enabled. */
export const DEFAULT_THINKING_BUDGET = 2048

export type ReasoningEffort = 'high' | 'max'

export function isQwenProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (p.id?.toLowerCase() === 'qwen') return true
  return /dashscope\.aliyuncs\.com|dashscope-intl\.aliyuncs\.com/i.test(p.baseUrl || '')
}

export function isDeepSeekProvider(p: Pick<ProviderConfig, 'id' | 'baseUrl'>): boolean {
  if (p.id?.toLowerCase() === 'deepseek') return true
  return /api\.deepseek\.com/i.test(p.baseUrl || '')
}

export function normalizeReasoningEffort(v: unknown): ReasoningEffort | undefined {
  if (v === 'high' || v === 'max') return v
  return undefined
}
