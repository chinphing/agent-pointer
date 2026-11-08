/**
 * 模型 payload 的读写转换。
 *
 * 抽出来是因为「字段名 / 取值」错一个，界面上看不出来，只有下发到客户端才失效：
 * - `thinkingProtocol` 客户端 `platform_config.rs` 里硬写成 None，配了不生效 → 不写
 * - `reasoningEffort` 只认 `high` / `max`，别的值会被 `normalize_reasoning_effort` 丢掉
 * - 留空必须是**不写这个键**，而不是写 null / 空串，否则客户端会当成显式设置
 */

export interface ModelFormState {
  name: string
  enabled: boolean
  caps: Record<string, string>
  limits: Record<string, string>
  thinking: Record<string, string>
}

/** 客户端认的模型级字段。多一个没有，少一个客户端读不到。 */
export const CAP_KEYS = ['supportsVision', 'supportsAudio', 'canGenerateImage', 'canGenerateVideo']
export const LIMIT_KEYS = ['contextBudgetTokens', 'maxTokens', 'temperature', 'topP']
/**
 * 思考只留「强度」和「是否回传 reasoning」两项。
 *
 * 另外三个都只是强度的派生结果，客户端设了强度就现算、把存的值覆盖掉：
 * - thinkingBudget  → budget_for_intensity(level)          thinking_strategy.rs:207
 * - enableThinking  → 强度非 off 即 true                    thinking_strategy.rs:419
 * - reasoningEffort → effort_for_intensity / deepseek 映射   thinking_strategy.rs:213
 *
 * 「回传 reasoning」是独立语义（要不要把 reasoning_content 回灌进消息），与强度无关。
 */
export const THINKING_KEYS = ['thinkingIntensity', 'reasoningInMessages']

/** 强度 → 预算的反查，只用于把历史数据里的「只有预算」迁移成强度。 */
const BUDGET_TO_INTENSITY: Array<[number, string]> = [
  [6144, 'max'],
  [3072, 'high'],
  [1536, 'medium'],
  [1, 'low'],
]
const BOOL_KEYS = new Set(['enableThinking', 'reasoningInMessages'])

/**
 * 从历史字段反推强度，顺序与客户端 `resolve_thinking_intensity` 一致：
 * 强度 → reasoningEffort（只认 high/max）→ 思考预算 → enableThinking。
 */
export function resolveIntensity(p: Record<string, unknown>): string {
  if (typeof p.thinkingIntensity === 'string' && p.thinkingIntensity) return p.thinkingIntensity
  const effort = typeof p.reasoningEffort === 'string' ? p.reasoningEffort : ''
  if (effort === 'high' || effort === 'max') return effort
  if (typeof p.thinkingBudget === 'number') return intensityFromBudget(p.thinkingBudget)
  if (p.enableThinking === false) return 'off'
  if (p.enableThinking === true) return 'medium'
  return ''
}

/** 预算反推强度，与客户端 intensity_from_budget 同一套分档。 */
export function intensityFromBudget(budget: number): string {
  if (budget <= 0) return 'off'
  for (const [floor, level] of BUDGET_TO_INTENSITY) {
    if (budget >= floor) return level
  }
  return 'low'
}

/** 三态布尔 → 表单值：'' = 跟随服务商。 */
export function toTri(v: unknown): string {
  if (v === true) return 'true'
  if (v === false) return 'false'
  return ''
}

/** payload → 表单状态。 */
export function payloadToState(payload: any) {
  const p = payload || {}
  const caps: Record<string, string> = {}
  for (const k of CAP_KEYS) caps[k] = toTri(p[k])
  const limits: Record<string, string> = {}
  for (const k of LIMIT_KEYS) limits[k] = p[k] === undefined || p[k] === null ? '' : String(p[k])
  const thinking: Record<string, string> = {}
  for (const k of THINKING_KEYS) {
    thinking[k] = BOOL_KEYS.has(k) ? toTri(p[k]) : p[k] === undefined || p[k] === null ? '' : String(p[k])
  }
  // 老数据可能只存了派生字段没存强度：按客户端 resolve_thinking_intensity 的顺序
  // 反推（强度 → effort → 预算 → enable），保存时归一到强度。
  thinking.thinkingIntensity = resolveIntensity(p)
  return { caps, limits, thinking }
}

/**
 * 表单状态 → payload。
 *
 * 只写填了的字段：留空表示「不下发，继承服务商默认」。数字串转数字，
 * 布尔三态转真布尔；无法解析的值直接丢弃，不往客户端送脏数据。
 */
export function buildModelPayload(state: {
  caps: Record<string, string>
  limits: Record<string, string>
  thinking: Record<string, string>
}): Record<string, unknown> {
  const out: Record<string, unknown> = {}

  for (const k of CAP_KEYS) {
    const v = state.caps[k]
    if (v === 'true') out[k] = true
    else if (v === 'false') out[k] = false
  }

  for (const k of LIMIT_KEYS) {
    const raw = state.limits[k]
    if (raw === '' || raw === undefined) continue
    const n = Number(raw)
    if (Number.isFinite(n)) out[k] = n
  }

  for (const k of THINKING_KEYS) {
    const raw = state.thinking[k]
    if (raw === '' || raw === undefined) continue
    if (BOOL_KEYS.has(k)) {
      if (raw === 'true') out[k] = true
      else if (raw === 'false') out[k] = false
      continue
    }
    if (k === 'reasoningEffort') {
      // 客户端只认这两个；别的值静默丢弃，不如在这里就不发。
      if (raw === 'high' || raw === 'max') out[k] = raw
      continue
    }
    out[k] = raw
  }

  return out
}

/** 服务商级默认参数（模型没单独配时用它）。能力不在此列——能力只有模型级。 */
export const PROVIDER_DEFAULT_LIMIT_KEYS = LIMIT_KEYS
export const PROVIDER_DEFAULT_THINKING_KEYS = THINKING_KEYS

export function buildProviderDefaults(state: {
  limits: Record<string, string>
  thinking: Record<string, string>
}): Record<string, unknown> {
  const out = buildModelPayload({ caps: {}, limits: state.limits, thinking: state.thinking })
  return out
}
