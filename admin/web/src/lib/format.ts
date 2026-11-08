/**
 * 展示层格式化。这些函数原先各自复制在 Resources/Usage/Audit 三个页面里，
 * 抽出来统一口径并单测（`when` 曾经有两份完全相同的实现）。
 */

/** 字节数 → 人类可读。0 / 空值显示 `—`，避免把「没有包」误显示成 `0 B`。 */
export function humanSize(bytes: number | null | undefined): string {
  if (!bytes) return '—'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/**
 * Unix 秒 → 本地时间串。后端统一存秒，不要传毫秒。
 *
 * 自己拼而不是 `toLocaleString('zh-CN')`：后者输出 `2026/11/8 12:45:39`，
 * 月日不补零，一列时间对不齐，看着像临时拼的字符串。
 */
export function when(ts: number | null | undefined): string {
  if (!ts) return '—'
  const d = new Date(ts * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/** 整数千分位。 */
export function fmtCount(n: number | null | undefined): string {
  return Number(n || 0).toLocaleString('zh-CN')
}

/** JSON 美化；不是合法 JSON 就原样返回（审计日志里可能是任意文本）。 */
export function prettyJson(raw: string | null | undefined): string {
  if (!raw) return ''
  try {
    return JSON.stringify(JSON.parse(raw), null, 2)
  } catch {
    return raw
  }
}
