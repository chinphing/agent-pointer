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
export const THINKING_KEYS = [
  'enableThinking',
  'thinkingIntensity',
  'thinkingBudget',
  'reasoningEffort',
  'reasoningInMessages',
]
const BOOL_KEYS = new Set(['enableThinking', 'reasoningInMessages'])

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
    if (k === 'thinkingBudget') {
      const n = Number(raw)
      if (Number.isFinite(n)) out[k] = n
      continue
    }
    out[k] = raw
  }

  return out
}
