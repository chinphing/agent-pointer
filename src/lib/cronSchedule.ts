/**
 * Cron schedule helpers for the automation UI. The backend uses a 6-field,
 * second-level cron expression (`sec min hour dom mon dow`), which most users
 * cannot write by hand. These helpers bridge a small set of friendly presets
 * to/from that raw expression, plus a human-readable description.
 *
 * Fields are interpreted in the user's local timezone by the backend
 * (`cron::Schedule::after(Local::now())`), so "daily 09:30" means local 09:30.
 */

import { t } from '../i18n'

/** Friendly schedule modes backed by generated cron / one-shot strings. */
export type CronMode =
  | 'onceIn'
  | 'onceAt'
  | 'everyMinute'
  | 'everyNMinutes'
  | 'everyNHours'
  | 'dailyAt'
  | 'weeklyAt'
  | 'monthlyAt'
  | 'custom'

/** Preset parameter shape. Only the fields relevant to `mode` are used. */
export interface CronPreset {
  mode: CronMode
  /** For onceIn: delay amount. */
  delayAmount?: number
  /** For onceIn: `m` | `h` | `d`. */
  delayUnit?: 'm' | 'h' | 'd'
  /** For onceAt: local datetime-local value `YYYY-MM-DDTHH:MM`. */
  atLocal?: string
  /** For everyNMinutes / everyNHours. */
  interval?: number
  /** For dailyAt / weeklyAt / monthlyAt: hour 0-23. */
  hour?: number
  /** For dailyAt / weeklyAt / monthlyAt: minute 0-59. */
  minute?: number
  /** For weeklyAt: 0=Sunday … 6=Saturday. */
  weekday?: number
  /** For monthlyAt: day of month 1-31. */
  dayOfMonth?: number
  /** For custom: the raw expression. */
  raw?: string
}

export function weekdayLabels(): string[] {
  return [0, 1, 2, 3, 4, 5, 6].map((i) => t(`settings.cron.weekday${i}`))
}

const DEFAULT_PRESET: CronPreset = { mode: 'dailyAt', hour: 9, minute: 0 }

/** Build a schedule string for create API (`schedule` field). */
export function buildCron(p: CronPreset): string {
  switch (p.mode) {
    case 'onceIn': {
      const n = clampInt(p.delayAmount, 1, 9999, 30)
      const u = p.delayUnit === 'h' || p.delayUnit === 'd' ? p.delayUnit : 'm'
      return `${n}${u}`
    }
    case 'onceAt': {
      const raw = (p.atLocal ?? '').trim()
      if (!raw) return ''
      // datetime-local is `YYYY-MM-DDTHH:MM`; append seconds for parser.
      return raw.length === 16 ? `${raw}:00` : raw
    }
    case 'everyMinute':
      return '0 * * * * *'
    case 'everyNMinutes': {
      const n = clampInt(p.interval, 1, 59, 5)
      return `0 */${n} * * * *`
    }
    case 'everyNHours': {
      const n = clampInt(p.interval, 1, 23, 2)
      return `0 0 */${n} * * *`
    }
    case 'dailyAt': {
      const h = clampInt(p.hour, 0, 23, 9)
      const m = clampInt(p.minute, 0, 59, 0)
      return `0 ${m} ${h} * * *`
    }
    case 'weeklyAt': {
      const h = clampInt(p.hour, 0, 23, 9)
      const m = clampInt(p.minute, 0, 59, 0)
      const d = clampInt(p.weekday, 0, 6, 1)
      return `0 ${m} ${h} * * ${d}`
    }
    case 'monthlyAt': {
      const h = clampInt(p.hour, 0, 23, 9)
      const m = clampInt(p.minute, 0, 59, 0)
      const dom = clampInt(p.dayOfMonth, 1, 31, 1)
      return `0 ${m} ${h} ${dom} * *`
    }
    case 'custom':
    default:
      return (p.raw ?? '').trim()
  }
}

/**
 * Best-effort reverse parse of a cron expression into a preset. Falls back to
 * `custom` (with the raw string preserved) for anything not matching a preset,
 * so editing an existing advanced job keeps its expression intact.
 */
export function parseCron(expr: string): CronPreset {
  const raw = (expr ?? '').trim()
  if (!raw) return { ...DEFAULT_PRESET }

  const onceRel = /^(\d+)([mhd])$/i.exec(raw)
  if (onceRel) {
    return {
      mode: 'onceIn',
      delayAmount: parseInt(onceRel[1], 10),
      delayUnit: onceRel[2].toLowerCase() as 'm' | 'h' | 'd'
    }
  }
  if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(raw) || /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}/.test(raw)) {
    const atLocal = raw.includes(' ')
      ? raw.replace(' ', 'T').slice(0, 16)
      : raw.slice(0, 16)
    return { mode: 'onceAt', atLocal }
  }

  const parts = raw.split(/\s+/)
  if (parts.length !== 6) return { mode: 'custom', raw }
  const [s, m, h, dom, mon, dow] = parts
  const star = (x: string) => x === '*'
  const num = (x: string) => (/^-?\d+$/.test(x) ? parseInt(x, 10) : null)
  const step = (x: string) => {
    const mt = /^\*\/(\d+)$/.exec(x)
    return mt ? parseInt(mt[1], 10) : null
  }
  if (s !== '0') return { mode: 'custom', raw }

  // everyMinute: 0 * * * * *
  if (star(m) && star(h) && star(dom) && star(mon) && star(dow)) {
    return { mode: 'everyMinute' }
  }
  // everyNMinutes: 0 */N * * * *
  const nMin = step(m)
  if (nMin != null && star(h) && star(dom) && star(mon) && star(dow)) {
    return { mode: 'everyNMinutes', interval: nMin }
  }
  // everyNHours: 0 0 */N * * *
  const nHour = step(h)
  if (m === '0' && nHour != null && star(dom) && star(mon) && star(dow)) {
    return { mode: 'everyNHours', interval: nHour }
  }
  // dailyAt / weeklyAt / monthlyAt: 0 M H [dom] * [dow]
  const mm = num(m)
  const hh = num(h)
  if (mm != null && mm >= 0 && mm <= 59 && hh != null && hh >= 0 && hh <= 23 && star(mon)) {
    // monthlyAt: 0 M H D * *  (day-of-month is a number, dow is *)
    const domNum = num(dom)
    if (domNum != null && domNum >= 1 && domNum <= 31 && star(dow)) {
      return { mode: 'monthlyAt', hour: hh, minute: mm, dayOfMonth: domNum }
    }
    if (star(dom) && star(dow)) {
      return { mode: 'dailyAt', hour: hh, minute: mm }
    }
    if (star(dom)) {
      const d = num(dow)
      if (d != null && d >= 0 && d <= 6) {
        return { mode: 'weeklyAt', hour: hh, minute: mm, weekday: d }
      }
    }
  }
  return { mode: 'custom', raw }
}

/** Human-readable description of a schedule string or cron expression. */
export function describeCron(expr: string): string {
  const p = parseCron(expr)
  const labels = weekdayLabels()
  switch (p.mode) {
    case 'onceIn': {
      const n = p.delayAmount ?? 30
      const u =
        p.delayUnit === 'h'
          ? t('settings.cron.hours')
          : p.delayUnit === 'd'
            ? t('settings.cron.days')
            : t('settings.cron.minutes')
      return t('settings.cron.onceIn', { amount: n, unit: u })
    }
    case 'onceAt':
      return t('settings.cron.onceAt', { when: p.atLocal ?? expr })
    case 'everyMinute':
      return t('settings.cron.everyMinute')
    case 'everyNMinutes':
      return t('settings.cron.everyNMinutes', { n: p.interval ?? 5 })
    case 'everyNHours':
      return t('settings.cron.everyNHours', { n: p.interval ?? 2 })
    case 'dailyAt':
      return t('settings.cron.dailyAt', {
        time: `${pad(p.hour ?? 9)}:${pad(p.minute ?? 0)}`
      })
    case 'weeklyAt':
      return t('settings.cron.weeklyAt', {
        weekday: labels[p.weekday ?? 1],
        time: `${pad(p.hour ?? 9)}:${pad(p.minute ?? 0)}`
      })
    case 'monthlyAt':
      return t('settings.cron.monthlyAt', {
        day: p.dayOfMonth ?? 1,
        time: `${pad(p.hour ?? 9)}:${pad(p.minute ?? 0)}`
      })
    case 'custom':
    default:
      return t('settings.cron.custom', { expr: (expr ?? '').trim() || '—' })
  }
}

/** Describe a stored cron job row (prefers scheduleKind / scheduleRaw). */
export function describeCronJob(job: {
  cronExpr: string
  scheduleKind?: string | null
  scheduleRaw?: string | null
  nextRunAtMs?: number | null
  enabled?: boolean
}): string {
  if (job.scheduleKind === 'once') {
    if (job.enabled === false && !job.nextRunAtMs) {
      const raw = job.scheduleRaw?.trim()
      return raw
        ? t('settings.cron.onceDoneWithTime', { when: raw })
        : t('settings.cron.onceDone')
    }
    if (job.nextRunAtMs) {
      return t('settings.cron.onceAtTime', {
        when: new Date(job.nextRunAtMs).toLocaleString()
      })
    }
    return describeCron(job.scheduleRaw || job.cronExpr)
  }
  return describeCron(job.scheduleRaw || job.cronExpr)
}

function clampInt(v: unknown, lo: number, hi: number, fallback: number): number {
  const n = typeof v === 'number' ? v : parseInt(String(v ?? ''), 10)
  if (!Number.isFinite(n)) return fallback
  return Math.min(hi, Math.max(lo, Math.trunc(n)))
}

function pad(n: number): string {
  return String(Math.trunc(n)).padStart(2, '0')
}

/** Local hour for cron session daily rollover (matches backend). */
export const CRON_SESSION_RESET_AT_HOUR = 4

/** Most recent daily reset boundary (ms) at `atHour` local, mirroring backend. */
export function dailyResetAtMs(now: Date, atHour: number = CRON_SESSION_RESET_AT_HOUR): number {
  const hour = Math.min(23, Math.max(0, atHour))
  const resetToday = new Date(now)
  resetToday.setHours(hour, 0, 0, 0)
  if (now.getTime() >= resetToday.getTime()) {
    return resetToday.getTime()
  }
  const resetYesterday = new Date(resetToday)
  resetYesterday.setDate(resetYesterday.getDate() - 1)
  return resetYesterday.getTime()
}

/** Active cron transcript session id at `at` (local reset window). */
export function currentCronSessionId(jobId: string, at: Date = new Date()): string {
  const boundary = new Date(dailyResetAtMs(at))
  const y = boundary.getFullYear()
  const m = pad(boundary.getMonth() + 1)
  const d = pad(boundary.getDate())
  return `cron:${jobId}:${y}${m}${d}`
}

/** Resolve the session id the UI should open for a cron job row. */
export function resolveCronViewSessionId(job: {
  id: string
  currentSessionId?: string | null
  lastRunAtMs?: number | null
}): string | null {
  const persisted = job.currentSessionId?.trim()
  if (persisted) return persisted
  if (job.lastRunAtMs) {
    return currentCronSessionId(job.id, new Date(job.lastRunAtMs))
  }
  return null
}
