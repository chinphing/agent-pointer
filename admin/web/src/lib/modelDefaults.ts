/**
 * 客户端（pointer-core）的真实默认值。
 *
 * 管理端所有「留空」最终都落到这些值上，所以界面必须按它们显示——
 * 否则管理员填了个数，以为自己改的是默认，其实客户端根本不这么算。
 *
 * 来源（改这里之前先改那边）：
 * - `crates/pointer-core/src/models/settings.rs:318-322`
 *     DEFAULT_MODEL_TEMPERATURE = 0.7
 *     DEFAULT_MODEL_TOP_P = 0.95
 *     DEFAULT_MODEL_MAX_TOKENS = 16_384
 *     DEFAULT_THINKING_BUDGET = 2_048
 * - `crates/pointer-core/src/models/settings.rs:1237`
 *     default_context_budget_tokens() = 256 * 1024
 * - `crates/pointer-core/src/models/settings.rs:315`
 *     reasoning_in_messages 默认 false
 * - `crates/pointer-core/src/models/settings.rs:3487`
 *     enable_thinking 默认 true
 * - `crates/pointer-core/src/thinking_strategy.rs:159-167`
 *     强度 → 思考预算：off 0 / low 1024 / medium 2048 / high 4096 / max 8192
 */

export const DEFAULT_TEMPERATURE = 0.7
export const DEFAULT_TOP_P = 0.95
export const DEFAULT_MAX_TOKENS = 16384
export const DEFAULT_CONTEXT_BUDGET_TOKENS = 262144
export const DEFAULT_THINKING_BUDGET = 2048
export const DEFAULT_ENABLE_THINKING = true
export const DEFAULT_REASONING_IN_MESSAGES = false

/** 产品统一的思考强度阶梯，以及它换算出的思考预算。 */
export const INTENSITY_BUDGET: Record<string, number> = {
  off: 0,
  low: 1024,
  medium: 2048,
  high: 4096,
  max: 8192,
}

/** 每个可留空字段的「默认是什么」，直接拿去做占位文案。 */
export const FIELD_DEFAULTS = {
  contextBudgetTokens: DEFAULT_CONTEXT_BUDGET_TOKENS,
  maxTokens: DEFAULT_MAX_TOKENS,
  temperature: DEFAULT_TEMPERATURE,
  topP: DEFAULT_TOP_P,
  thinkingBudget: DEFAULT_THINKING_BUDGET,
} as const

export function contextBudgetLabel() {
  return `${DEFAULT_CONTEXT_BUDGET_TOKENS}（256K）`
}
